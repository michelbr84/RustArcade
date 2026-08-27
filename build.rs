//! Embeds every manifest under `catalog/games/*.toml` into the binary so RustArcade
//! always ships a working built-in catalog, even when offline.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let games_dir = Path::new(&manifest_dir).join("catalog").join("games");
    println!("cargo:rerun-if-changed=catalog/games");

    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(&games_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                println!("cargo:rerun-if-changed={}", path.display());
                files.push(path);
            }
        }
    }
    files.sort();

    let mut source = String::from(
        "/// Built-in catalog manifests: `(file name, TOML source)`, sorted by file name.\n\
         pub static BUILTIN_MANIFESTS: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("manifest file names are UTF-8");
        source.push_str(&format!(
            "    ({name:?}, include_str!({:?})),\n",
            path.display().to_string()
        ));
    }
    source.push_str("];\n");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    fs::write(Path::new(&out_dir).join("builtin_catalog.rs"), source)
        .expect("write generated builtin catalog");
}
