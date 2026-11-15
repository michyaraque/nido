use std::{env, fs, path::Path};

include!("tools/css.rs");

fn files(dir: &Path, ext: Option<&str>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| ext.map_or(true, |x| p.extension().and_then(|e| e.to_str()) == Some(x)))
                .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p.to_str()?.replace('\\', "/"))))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn main() {
    println!("cargo:rerun-if-changed=assets/logos");
    println!("cargo:rerun-if-changed=assets/icons");
    println!("cargo:rerun-if-changed=assets/css");
    println!("cargo:rerun-if-changed=tools/css.rs");
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    let out = Path::new(&env::var("OUT_DIR").unwrap()).to_path_buf();

    let mut s = String::from("pub static LOGOS: &[(&str, &[u8])] = &[\n");
    for (slug, path) in files(&root.join("logos"), None) {
        s += &format!("    ({slug:?}, include_bytes!({path:?})),\n");
    }
    fs::write(out.join("logos.rs"), s + "];\n").unwrap();

    let mut s = String::from("pub static ICONS: &[(&str, &str)] = &[\n");
    for (name, path) in files(&root.join("icons"), Some("svg")) {
        let svg = fs::read_to_string(&path).unwrap();
        let start = svg.find("<svg").and_then(|i| svg[i..].find('>').map(|j| i + j + 1)).unwrap();
        let end = svg.rfind("</svg>").unwrap();
        let inner = svg[start..end].split_whitespace().collect::<Vec<_>>().join(" ");
        s += &format!("    ({name:?}, {inner:?}),\n");
    }
    fs::write(out.join("icons.rs"), s + "];\n").unwrap();

    let css: String = ["tokens", "base", "layout", "components", "pages"]
        .iter()
        .map(|n| fs::read_to_string(root.join("css").join(format!("{n}.css"))).unwrap())
        .collect();
    fs::write(out.join("style.css"), minify_css(&css)).unwrap();

    // Board family, as release assets name it: extras are downloaded for the same one.
    let target = env::var("TARGET").unwrap();
    let arch = if target.starts_with("aarch64") { "aarch64" } else if target.starts_with("armv7") { "armv7" } else if target.starts_with("arm") { "armv6" } else { "other" };
    println!("cargo:rustc-env=NIDO_ARCH={arch}");
}
