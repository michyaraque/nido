//! The extra draws its icons with the same Lucide set as Nido (assets/icons), so its page fits in.
use std::{env, fs, path::Path};

fn main() {
    let icons = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/icons");
    println!("cargo:rerun-if-changed={}", icons.display());
    let mut list: Vec<_> = fs::read_dir(&icons)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    list.sort();
    let mut s = String::from("pub static ICONS: &[(&str, &str)] = &[\n");
    for p in list {
        let svg = fs::read_to_string(&p).unwrap();
        let start = svg
            .find("<svg")
            .and_then(|i| svg[i..].find('>').map(|j| i + j + 1))
            .unwrap();
        let inner = svg[start..svg.rfind("</svg>").unwrap()]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        s += &format!(
            "    ({:?}, {inner:?}),\n",
            p.file_stem().unwrap().to_str().unwrap()
        );
    }
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("icons.rs"),
        s + "];\n",
    )
    .unwrap();
}
