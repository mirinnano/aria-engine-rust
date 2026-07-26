use std::fs;
use std::path::Path;

fn watch_tree(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_dir() {
            watch_tree(&child);
        } else {
            println!("cargo:rerun-if-changed={}", child.display());
        }
    }
}

fn main() {
    // Tauri embeds `frontendDist` for the native shell. The default generated
    // build script only watches tauri.conf.json, so a fresh React/package build
    // could otherwise leave `tauri dev` serving an obsolete (or empty) asset
    // snapshot. Keep every packaged web asset in Cargo's dependency graph.
    watch_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/web"));
    tauri_build::build()
}
