use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader, RgbImage};
use rten::{Model, ValueOrView};
use rten_tensor::prelude::*;
use rten_tensor::NdTensor;
use std::{io::Cursor, path::PathBuf, sync::Mutex, sync::OnceLock};

const MAX_SIDE: u32 = 1600;
const DET_SIDE: u32 = 960;
const REC_HEIGHT: usize = 48;
pub const DETECTION: &str = "pp-ocrv6_tiny_det.onnx";
pub const RECOGNITION: &str = "pp-ocrv6_tiny_rec.onnx";
pub const DICTIONARY: &str = "ppocrv6_tiny_dict.txt";

fn models_dir() -> PathBuf {
    std::env::var_os("NIDO_MODELS").map(PathBuf::from).unwrap_or_else(|| "models".into())
}

pub fn installed() -> bool {
    let dir = models_dir();
    [DETECTION, RECOGNITION, DICTIONARY].iter().all(|f| dir.join(f).exists())
}

struct Engine {
    detector: Model,
    recognizer: Model,
    chars: Vec<String>,
}

/// One engine for the whole process; the mutex also keeps memory bounded to one photo at a time.
fn engine() -> Option<&'static Mutex<Engine>> {
    static ENGINE: OnceLock<Option<Mutex<Engine>>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let dir = models_dir();
            let load = |name: &str| Model::load_file(dir.join(name)).map_err(|e| eprintln!("ocr: {name}: {e}")).ok();
            let chars = std::fs::read_to_string(dir.join(DICTIONARY))
                .map_err(|e| eprintln!("ocr: {DICTIONARY}: {e}"))
                .ok()?
                .lines()
                .map(str::to_string)
                .collect();
            Some(Mutex::new(Engine { detector: load(DETECTION)?, recognizer: load(RECOGNITION)?, chars }))
        })
        .as_ref()
}

pub fn warm_up() -> bool {
    engine().is_some()
}

pub fn normalize(photo: &[u8]) -> Result<(DynamicImage, Vec<u8>), String> {
    let mut decoder = ImageReader::new(Cursor::new(photo))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|_| "El archivo no es una imagen que podamos leer (usa JPG, PNG o WebP).".to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    if img.width().max(img.height()) > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle);
    }
    // The copy we keep: grey WebP at quality 55. A receipt is black on white, so colour adds nothing,
    // and it stays perfectly readable at about a third of the size of a colour JPEG (~150 KB).
    let grey = DynamicImage::ImageLuma8(img.to_luma8()).to_rgb8();
    let stored = webp::Encoder::from_rgb(&grey, grey.width(), grey.height()).encode(55.0).to_vec();
    Ok((img, stored))
}

struct Line {
    cx: f32,
    cy: f32,
    ux: f32,
    uy: f32,
    half_w: f32,
    half_h: f32,
}

fn run(model: &Model, input: NdTensor<f32, 4>) -> Result<NdTensor<f32, 4>, String> {
    let id = model.input_ids()[0];
    let out = model
        .run(vec![(id, ValueOrView::from(input.view()))], &[model.output_ids()[0]], None)
        .map_err(|e| e.to_string())?;
    let out: rten_tensor::Tensor<f32> = out.into_iter().next().ok_or("sin salida")?.try_into().map_err(|_| "salida inesperada")?;
    let shape = out.shape().to_vec();
    let shape4 = match shape.as_slice() {
        [a, b, c, d] => [*a, *b, *c, *d],
        [a, b, c] => [1, *a, *b, *c],
        _ => return Err(format!("forma de salida inesperada {shape:?}")),
    };
    Ok(out.into_shape(shape4))
}

fn detect(model: &Model, img: &RgbImage) -> Result<Vec<Line>, String> {
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let (w, h) = img.dimensions();
    let scale = (DET_SIDE as f32 / w.max(h) as f32).min(1.0);
    let rw = ((w as f32 * scale / 32.0).round().max(1.0) as u32) * 32;
    let rh = ((h as f32 * scale / 32.0).round().max(1.0) as u32) * 32;
    let small = image::imageops::resize(img, rw, rh, FilterType::Triangle);
    let mut input = NdTensor::<f32, 4>::zeros([1, 3, rh as usize, rw as usize]);
    for (x, y, p) in small.enumerate_pixels() {
        // The models were trained on BGR images.
        for (c, v) in [p[2], p[1], p[0]].into_iter().enumerate() {
            input[[0, c, y as usize, x as usize]] = (v as f32 / 255.0 - MEAN[c]) / STD[c];
        }
    }
    let prob = run(model, input)?;
    let (pw, ph) = (rw as usize, rh as usize);
    let at = |x: usize, y: usize| prob[[0, 0, y, x]];

    let mut seen = vec![false; pw * ph];
    let mut blobs: Vec<Vec<(usize, usize)>> = Vec::new();
    for start in 0..pw * ph {
        if seen[start] || at(start % pw, start / pw) <= 0.3 {
            continue;
        }
        let mut blob = Vec::new();
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % pw, i / pw);
            blob.push((x, y));
            let neighbours = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
            for (nx, ny) in neighbours {
                if nx < pw && ny < ph {
                    let j = ny * pw + nx;
                    if !seen[j] && at(nx, ny) > 0.3 {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let score = blob.iter().map(|&(x, y)| at(x, y)).sum::<f32>() / blob.len() as f32;
        if blob.len() >= 12 && score >= 0.6 {
            blobs.push(blob);
        }
    }

    let direction = |b: &[(usize, usize)]| {
        let n = b.len() as f32;
        let (mx, my) = b.iter().fold((0.0, 0.0), |(sx, sy), &(x, y)| (sx + x as f32, sy + y as f32));
        let (mx, my) = (mx / n, my / n);
        let (mut sxx, mut syy, mut sxy) = (0.0f32, 0.0f32, 0.0f32);
        for &(x, y) in b {
            let (dx, dy) = (x as f32 - mx, y as f32 - my);
            sxx += dx * dx;
            syy += dy * dy;
            sxy += dx * dy;
        }
        (0.5 * (2.0 * sxy).atan2(sxx - syy), sxx / syy.max(1.0))
    };
    let mut angles: Vec<f32> =
        blobs.iter().map(|b| direction(b)).filter(|(a, ratio)| *ratio > 9.0 && a.abs() < 0.6).map(|(a, _)| a).collect();
    angles.sort_by(f32::total_cmp);
    let angle = angles.get(angles.len() / 2).copied().unwrap_or(0.0);
    let (ux, uy) = (angle.cos(), angle.sin());

    let to_uv = |&(x, y): &(usize, usize)| {
        let (x, y) = (x as f32, y as f32);
        (x * ux + y * uy, -x * uy + y * ux)
    };
    let height = |pts: &[(f32, f32)]| {
        let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
        hi - lo + 1.0
    };
    let uv: Vec<Vec<(f32, f32)>> = blobs.iter().map(|b| b.iter().map(to_uv).collect()).collect();
    let mut heights: Vec<f32> = uv.iter().map(|p| height(p)).collect();
    heights.sort_by(f32::total_cmp);
    let line_h = heights.get(heights.len() / 2).copied().unwrap_or(10.0);

    // Tightly printed receipts make neighbouring lines touch; split tall blobs at the gaps between rows.
    let mut pieces: Vec<Vec<(f32, f32)>> = Vec::new();
    for (blob, pts) in blobs.iter().zip(uv) {
        if height(&pts) < 1.4 * line_h {
            pieces.push(pts);
            continue;
        }
        let (a, _) = direction(blob);
        let a = if a.abs() < 0.6 { a } else { angle };
        let across: Vec<f32> = blob.iter().map(|&(x, y)| -(x as f32) * a.sin() + y as f32 * a.cos()).collect();
        let v0 = across.iter().copied().fold(f32::MAX, f32::min);
        let span = across.iter().copied().fold(f32::MIN, f32::max) - v0;
        let mut profile = vec![0usize; span as usize + 2];
        for v in &across {
            profile[(v - v0) as usize] += 1;
        }
        let peak = *profile.iter().max().unwrap_or(&1);
        let mut bands: Vec<(usize, usize)> = Vec::new();
        let mut start = None;
        for (i, &n) in profile.iter().enumerate() {
            match (n * 5 > peak, start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    bands.push((s, i));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            bands.push((s, profile.len()));
        }
        for (lo, hi) in bands {
            let piece: Vec<(f32, f32)> = pts
                .iter()
                .zip(&across)
                .filter(|(_, v)| (lo..hi).contains(&((*v - v0) as usize)))
                .map(|(p, _)| *p)
                .collect();
            if piece.len() >= 12 {
                pieces.push(piece);
            }
        }
    }

    let (sx, sy) = (w as f32 / rw as f32, h as f32 / rh as f32);
    Ok(pieces
        .iter()
        .map(|pts| {
            let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for &(u, v) in pts {
                u0 = u0.min(u);
                u1 = u1.max(u + 1.0);
                v0 = v0.min(v);
                v1 = v1.max(v + 1.0);
            }
            let (bw, bh) = (u1 - u0, v1 - v0);
            // The detector draws text a little thinner than it is ("unclip": area / perimeter × 1.5).
            let grow = bw * bh * 1.5 / (2.0 * (bw + bh));
            let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
            let (cx, cy) = (cu * ux - cv * uy, cu * uy + cv * ux);
            Line { cx: cx * sx, cy: cy * sy, ux, uy, half_w: (bw / 2.0 + grow) * sx, half_h: (bh / 2.0 + grow) * sy }
        })
        .collect())
}

fn crop(img: &RgbImage, l: &Line) -> NdTensor<f32, 4> {
    let (w, h) = (2.0 * l.half_w, 2.0 * l.half_h);
    let out_w = ((REC_HEIGHT as f32 * w / h).round() as usize).clamp(16, 2400);
    let mut t = NdTensor::<f32, 4>::zeros([1, 3, REC_HEIGHT, out_w]);
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    let sample = |x: f32, y: f32| -> [f32; 3] {
        let (x, y) = (x.clamp(0.0, iw - 1.0), y.clamp(0.0, ih - 1.0));
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let px = |dx: f32, dy: f32| img.get_pixel((x0 + dx).min(iw - 1.0) as u32, (y0 + dy).min(ih - 1.0) as u32).0;
        let (a, b, c, d) = (px(0.0, 0.0), px(1.0, 0.0), px(0.0, 1.0), px(1.0, 1.0));
        let mut out = [0.0; 3];
        for k in 0..3 {
            let top = a[k] as f32 * (1.0 - fx) + b[k] as f32 * fx;
            let bottom = c[k] as f32 * (1.0 - fx) + d[k] as f32 * fx;
            out[k] = top * (1.0 - fy) + bottom * fy;
        }
        out
    };
    for oy in 0..REC_HEIGHT {
        let v = (oy as f32 + 0.5) / REC_HEIGHT as f32 * h - l.half_h;
        for ox in 0..out_w {
            let u = (ox as f32 + 0.5) / out_w as f32 * w - l.half_w;
            let rgb = sample(l.cx + u * l.ux - v * l.uy, l.cy + u * l.uy + v * l.ux);
            for (c, val) in [rgb[2], rgb[1], rgb[0]].into_iter().enumerate() {
                t[[0, c, oy, ox]] = (val / 255.0 - 0.5) / 0.5;
            }
        }
    }
    t
}

fn decode(probs: &NdTensor<f32, 4>, chars: &[String]) -> (String, f32) {
    let [_, _, steps, classes] = probs.shape();
    let (mut text, mut last, mut conf, mut n) = (String::new(), 0, 0.0, 0);
    for t in 0..steps {
        let (best, p) = (0..classes).map(|c| (c, probs[[0, 0, t, c]])).fold((0, f32::MIN), |a, b| if b.1 > a.1 { b } else { a });
        if best != 0 && best != last {
            // Class 0 is "blank"; one past the dictionary is the space.
            text.push_str(chars.get(best - 1).map(String::as_str).unwrap_or(" "));
            conf += p;
            n += 1;
        }
        last = best;
    }
    (text.trim().to_string(), if n > 0 { conf / n as f32 } else { 0.0 })
}

pub fn read_lines(img: &DynamicImage) -> Result<Vec<String>, String> {
    let engine = engine().ok_or("El lector de tickets no está instalado en este servidor.")?;
    let engine = engine.lock().unwrap_or_else(|e| e.into_inner());
    let rgb = img.to_rgb8();
    let mut segments = Vec::new();
    for l in detect(&engine.detector, &rgb)? {
        let probs = run(&engine.recognizer, crop(&rgb, &l))?;
        let (text, conf) = decode(&probs, &engine.chars);
        if conf < 0.5 || !text.chars().any(char::is_alphanumeric) {
            continue;
        }
        segments.push(Segment {
            text,
            x: l.cx - l.ux * l.half_w,
            y: l.cy - l.uy * l.half_w,
            end: (l.cx + l.ux * l.half_w, l.cy + l.uy * l.half_w),
            height: 2.0 * l.half_h,
        });
    }
    Ok(rows(segments))
}

struct Segment {
    text: String,
    x: f32,
    y: f32,
    end: (f32, f32),
    height: f32,
}

fn tilt(segments: &[Segment]) -> f32 {
    let mut slopes: Vec<f32> = segments
        .iter()
        .filter(|s| s.end.0 - s.x > 4.0 * s.height)
        .map(|s| (s.end.1 - s.y) / (s.end.0 - s.x))
        .collect();
    slopes.sort_by(f32::total_cmp);
    slopes.get(slopes.len() / 2).copied().unwrap_or(0.0)
}

fn rows(mut segments: Vec<Segment>) -> Vec<String> {
    let slope = tilt(&segments);
    let level = |s: &Segment| (s.y + s.end.1) / 2.0 - slope * (s.x + s.end.0) / 2.0;
    segments.sort_by(|a, b| level(a).total_cmp(&level(b)));
    let mut rows: Vec<Vec<Segment>> = Vec::new();
    for s in segments {
        match rows.last_mut() {
            Some(row) if (level(&s) - level(&row[0])).abs() < 0.5 * row[0].height.max(s.height) => row.push(s),
            _ => rows.push(vec![s]),
        }
    }
    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            row.into_iter().map(|s| s.text).collect::<Vec<_>>().join("  ")
        })
        .collect()
}
