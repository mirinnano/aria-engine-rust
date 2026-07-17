use std::fs;
use std::path::Path;

#[test]
fn core_has_no_platform_or_device_dependencies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    for forbidden in [
        "wgpu",
        "winit",
        "web-sys",
        "web_sys",
        "cpal",
        "kira",
        "accesskit",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "aria-core must not depend on {forbidden}"
        );
    }

    let mut sources = Vec::new();
    collect_rs_files(&root.join("src"), &mut sources);
    for source_path in sources {
        let source = fs::read_to_string(&source_path).unwrap();
        for forbidden in [
            "std::fs",
            "std::net",
            "std::path",
            "std::process",
            "std::time::Instant",
            "std::time::SystemTime",
            "web_sys::",
            "wgpu::",
            "winit::",
            "cpal::",
            "kira::",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} exposes platform capability {forbidden}",
                source_path.display()
            );
        }
    }
}

fn collect_rs_files(directory: &Path, output: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}
