use std::{env, fs, path::Path};

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
}
