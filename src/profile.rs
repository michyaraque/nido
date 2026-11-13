use super::*;
use axum::extract::Multipart;
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader};
use std::{io::Cursor, path::PathBuf};

const SIDE: u32 = 256;

pub fn path(id: i64) -> PathBuf {
    data_dir().join("avatars").join(format!("{id}.jpg"))
}

pub fn square_jpeg(photo: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = ImageReader::new(Cursor::new(photo))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|_| "El archivo no es una imagen que podamos leer.".to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let side = img.width().min(img.height());
    let img = img
        .crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side)
        .resize_exact(SIDE, SIDE, FilterType::Lanczos3);
    let mut jpeg = Vec::new();
    img.to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85))
        .map_err(|e| e.to_string())?;
    Ok(jpeg)
}

pub async fn upload_avatar(State(db): State<Db>, h: HeaderMap, mut form: Multipart) -> R {
    let me = {
        let c = lock(&db);
        require_user!(c, h)
    };
    let mut photo = Vec::new();
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() == Some("photo") {
            photo = field.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
        }
    }
    if photo.is_empty() {
        return bad("Elige una foto.");
    }
    let jpeg = match tokio::task::spawn_blocking(move || square_jpeg(&photo)).await? {
        Ok(j) => j,
        Err(e) => return Ok((StatusCode::BAD_REQUEST, e).into_response()),
    };
    let p = path(me.id);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, jpeg)?;
    let c = lock(&db);
    c.execute("UPDATE users SET photo=photo+1 WHERE id=?1", [me.id])?;
    back(&month(&c, None)?, Some("settings"), "photo_saved")
}

pub async fn delete_avatar(State(db): State<Db>, h: HeaderMap) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let _ = std::fs::remove_file(path(me.id));
    c.execute("UPDATE users SET photo=0 WHERE id=?1", [me.id])?;
    back(&month(&c, None)?, Some("settings"), "photo_removed")
}

pub async fn avatar(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    {
        let c = lock(&db);
        require_user!(c, h);
    }
    match std::fs::read(path(id)) {
        Ok(b) => Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=31536000, immutable")], b).into_response()),
        Err(_) => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}
