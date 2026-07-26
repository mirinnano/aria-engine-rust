//! Validation and copying of target runtime artifacts for distributable builds.
//!
//! The compiler deliberately knows nothing about executables or browser glue.
//! This module is the small packaging boundary that makes a `--release` build
//! fail closed instead of producing a directory that merely looks playable.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use crate::build::BuildTarget;
use crate::project::logical_path;

/// Finds a Player suitable for `target`.
///
/// Development builds may omit a non-host Player. Release builds may not: the
/// caller must either be a release-profile CLI on the matching host or supply
/// an already-built target Player through `ARIA_PLAYER_BINARY`.
pub(crate) fn resolve_native_player(target: BuildTarget, release: bool) -> Result<Option<PathBuf>> {
    if target == BuildTarget::Web {
        return Ok(None);
    }

    let supplied = std::env::var_os("ARIA_PLAYER_BINARY").map(PathBuf::from);
    resolve_native_player_with(
        target,
        release,
        supplied,
        std::env::current_exe().ok(),
        host_supports(target),
        cfg!(debug_assertions),
    )
}

fn resolve_native_player_with(
    target: BuildTarget,
    release: bool,
    supplied: Option<PathBuf>,
    current_executable: Option<PathBuf>,
    host_matches_target: bool,
    debug_build: bool,
) -> Result<Option<PathBuf>> {
    if let Some(path) = supplied {
        return validate_native_player(&path, target).map(Some);
    }

    if !host_matches_target {
        if release {
            bail!(
                "release build for {} requires a matching Player; set ARIA_PLAYER_BINARY to a release Player built for that target",
                target.as_str()
            );
        }
        return Ok(None);
    }

    if release && debug_build {
        bail!(
            "refusing to package the current debug aria executable as a release Player; run `cargo run --release -p aria-cli -- build … --release` or set ARIA_PLAYER_BINARY to a matching release Player"
        );
    }

    let path = current_executable.context("cannot locate the current aria executable")?;
    validate_native_player(&path, target).map(Some)
}

/// The executable name written into the package is determined by the package
/// target, never the build host. This matters for a Windows package assembled
/// from an externally supplied Player on Linux CI.
#[must_use]
pub(crate) const fn native_player_filename(target: BuildTarget) -> &'static str {
    match target {
        BuildTarget::WindowsX64 => "aria-player.exe",
        BuildTarget::MacosUniversal
        | BuildTarget::LinuxX64
        | BuildTarget::SteamdeckX64
        | BuildTarget::Web => "aria-player",
    }
}

/// Copies a previously validated Player and preserves Unix executable bits.
pub(crate) fn copy_native_player(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination).with_context(|| {
        format!(
            "cannot copy Player {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    #[cfg(unix)]
    {
        let permissions = fs::metadata(source)
            .with_context(|| format!("cannot inspect Player {}", source.display()))?
            .permissions();
        fs::set_permissions(destination, permissions).with_context(|| {
            format!(
                "cannot preserve executable permissions for {}",
                destination.display()
            )
        })?;
    }
    Ok(())
}

/// Finds an optional wasm-bindgen package. The caller decides whether absence
/// is acceptable (development shell) or a release error.
pub(crate) fn discover_web_runtime_package() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ARIA_WEB_RUNTIME_DIR") {
        return Some(PathBuf::from(path));
    }
    let executable = std::env::current_exe().ok()?;
    let parent = executable.parent()?;
    [
        parent.join("web-runtime"),
        parent.join("share/aria/web-runtime"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_dir())
}

/// Resolves and preflights the web runtime before the build starts replacing
/// an output directory. A release has no meaningful fallback: an HTML shell
/// without the wasm-bindgen package is not a Player.
pub(crate) fn resolve_web_runtime(release: bool) -> Result<Option<PathBuf>> {
    let Some(path) = discover_web_runtime_package() else {
        if release {
            bail!(
                "release web build requires a wasm-bindgen runtime package; set ARIA_WEB_RUNTIME_DIR or use scripts/build-v3-web.sh"
            );
        }
        return Ok(None);
    };
    validate_web_runtime_package(&path)?;
    Ok(Some(path))
}

/// Recursively copies a wasm-bindgen package and returns canonical relative
/// paths for Service Worker precaching.
///
/// A package is accepted only when its JS glue and WASM module match the
/// Player's import contract. This is intentionally a small static check; the
/// Chromium gate executes the result as the final runtime proof.
pub(crate) fn copy_web_runtime(source: &Path, destination: &Path) -> Result<Vec<String>> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("cannot inspect web runtime {}", source.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "ARIA_WEB_RUNTIME_DIR must be a real directory: {}",
            source.display()
        );
    }

    validate_web_runtime_package(source)?;
    let mut files = Vec::new();
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!(
                "web runtime package must not contain symbolic links: {}",
                entry.path().display()
            );
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let name = logical_path(source, entry.path())?;
        let output = name
            .split('/')
            .fold(destination.to_owned(), |path, component| {
                path.join(component)
            });
        let parent = output
            .parent()
            .context("web runtime output has no parent directory")?;
        fs::create_dir_all(parent)?;
        fs::copy(entry.path(), &output).with_context(|| {
            format!(
                "cannot copy web runtime file {} to {}",
                entry.path().display(),
                output.display()
            )
        })?;
        files.push(name);
    }
    files.sort();
    if files.is_empty() {
        bail!(
            "web runtime package contains no files: {}",
            source.display()
        );
    }
    Ok(files)
}

pub(crate) fn validate_native_player(path: &Path, target: BuildTarget) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("cannot resolve Player binary {}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .with_context(|| format!("cannot inspect Player binary {}", canonical.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        bail!(
            "Player binary must be a non-empty regular file: {}",
            canonical.display()
        );
    }

    let mut bytes = Vec::new();
    fs::File::open(&canonical)
        .with_context(|| format!("cannot read Player binary {}", canonical.display()))?
        .take(8192)
        .read_to_end(&mut bytes)?;
    let valid = match target {
        BuildTarget::WindowsX64 => is_windows_x64_pe(&bytes),
        BuildTarget::LinuxX64 | BuildTarget::SteamdeckX64 => is_linux_x64_elf(&bytes),
        BuildTarget::MacosUniversal => is_macos_universal_macho(&bytes),
        BuildTarget::Web => false,
    };
    if !valid {
        bail!(
            "Player binary {} is not a valid {} Player artifact",
            canonical.display(),
            target.as_str()
        );
    }
    Ok(canonical)
}

pub(crate) fn validate_web_runtime_package(source: &Path) -> Result<()> {
    let javascript_path = source.join("aria_web.js");
    let wasm_path = source.join("aria_web_bg.wasm");
    if !javascript_path.is_file() || !wasm_path.is_file() {
        bail!(
            "web runtime package must contain aria_web.js and aria_web_bg.wasm: {}",
            source.display()
        );
    }
    let javascript = fs::read_to_string(&javascript_path)
        .with_context(|| format!("cannot read web runtime glue {}", javascript_path.display()))?;
    for required in [
        "aria_web_bg.wasm",
        "export class WebRuntime",
        "export class WebPak",
    ] {
        if !javascript.contains(required) {
            bail!(
                "web runtime glue {} is missing required wasm-bindgen Player export/reference '{required}'",
                javascript_path.display()
            );
        }
    }
    // wasm-bindgen 0.2.126 emits `__wbg_init as default`, while hand-written
    // compatible glue may use `export default ...`; both are the same ES
    // module contract consumed by the PWA shell.
    if !javascript.contains("export default") && !javascript.contains("as default") {
        bail!(
            "web runtime glue {} is missing a default wasm-bindgen initializer export",
            javascript_path.display()
        );
    }
    let mut wasm_header = [0_u8; 8];
    let count = fs::File::open(&wasm_path)
        .with_context(|| format!("cannot read web runtime WASM {}", wasm_path.display()))?
        .read(&mut wasm_header)?;
    if count != wasm_header.len() || wasm_header != [0, 0x61, 0x73, 0x6d, 1, 0, 0, 0] {
        bail!(
            "web runtime module {} is not a WebAssembly binary generated for this Player",
            wasm_path.display()
        );
    }
    Ok(())
}

fn host_supports(target: BuildTarget) -> bool {
    match target {
        BuildTarget::WindowsX64 => cfg!(target_os = "windows") && cfg!(target_arch = "x86_64"),
        // A universal macOS Player must be assembled from Intel and Apple
        // Silicon artifacts. A host executable must never be relabeled as one.
        BuildTarget::MacosUniversal => false,
        BuildTarget::LinuxX64 | BuildTarget::SteamdeckX64 => {
            cfg!(target_os = "linux") && cfg!(target_arch = "x86_64")
        }
        BuildTarget::Web => false,
    }
}

fn is_windows_x64_pe(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let offset = usize::try_from(u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()))
        .unwrap_or(usize::MAX);
    bytes.get(offset..offset.saturating_add(6)) == Some(b"PE\0\0\x64\x86".as_slice())
}

fn is_linux_x64_elf(bytes: &[u8]) -> bool {
    bytes.len() >= 20
        && bytes[..4] == [0x7f, b'E', b'L', b'F']
        && bytes[4] == 2 // ELFCLASS64
        && bytes[5] == 1 // little endian
        && bytes[18..20] == 0x3e_u16.to_le_bytes() // EM_X86_64
}

fn is_macos_universal_macho(bytes: &[u8]) -> bool {
    // `lipo -create` writes a big-endian FAT header. Supporting that canonical
    // representation keeps the cross-host contract strict and unambiguous.
    if bytes.len() < 8 || u32::from_be_bytes(bytes[..4].try_into().unwrap()) != 0xcafe_babe {
        return false;
    }
    let count =
        usize::try_from(u32::from_be_bytes(bytes[4..8].try_into().unwrap())).unwrap_or(usize::MAX);
    let Some(table_size) = count.checked_mul(20) else {
        return false;
    };
    if bytes.len() < 8 + table_size {
        return false;
    }
    let mut has_x86_64 = false;
    let mut has_arm64 = false;
    for index in 0..count {
        let start = 8 + index * 20;
        let cpu = u32::from_be_bytes(bytes[start..start + 4].try_into().unwrap());
        has_x86_64 |= cpu == 0x0100_0007;
        has_arm64 |= cpu == 0x0100_000c;
    }
    has_x86_64 && has_arm64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_refuses_a_debug_current_executable() {
        let error = resolve_native_player_with(
            BuildTarget::LinuxX64,
            true,
            None,
            Some(PathBuf::from("current-debug-player")),
            true,
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("debug aria executable"));
    }

    #[test]
    fn a_non_host_development_build_can_remain_a_data_only_artifact() {
        assert!(
            resolve_native_player_with(BuildTarget::WindowsX64, false, None, None, false, true,)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn release_non_host_build_requires_an_explicit_player() {
        let error =
            resolve_native_player_with(BuildTarget::WindowsX64, true, None, None, false, false)
                .unwrap_err();
        assert!(error.to_string().contains("ARIA_PLAYER_BINARY"));
    }

    #[test]
    fn recognizes_target_binary_headers() {
        let mut elf = vec![0; 20];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[18..20].copy_from_slice(&0x3e_u16.to_le_bytes());
        assert!(is_linux_x64_elf(&elf));

        let mut pe = vec![0; 0x90];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        pe[0x80..0x86].copy_from_slice(b"PE\0\0\x64\x86");
        assert!(is_windows_x64_pe(&pe));
    }

    #[test]
    fn recognizes_a_canonical_universal_macos_header() {
        let mut fat = vec![0; 48];
        fat[..4].copy_from_slice(&0xcafe_babe_u32.to_be_bytes());
        fat[4..8].copy_from_slice(&2_u32.to_be_bytes());
        fat[8..12].copy_from_slice(&0x0100_0007_u32.to_be_bytes());
        fat[28..32].copy_from_slice(&0x0100_000c_u32.to_be_bytes());
        assert!(is_macos_universal_macho(&fat));
    }

    #[test]
    fn validates_a_wasm_bindgen_runtime_contract() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("aria_web.js"),
            "export default function init() {} export class WebRuntime {} export class WebPak {} const wasm = 'aria_web_bg.wasm';",
        )
        .unwrap();
        fs::write(
            temp.path().join("aria_web_bg.wasm"),
            [0, 0x61, 0x73, 0x6d, 1, 0, 0, 0],
        )
        .unwrap();
        validate_web_runtime_package(temp.path()).unwrap();
    }

    #[test]
    fn accepts_the_wasm_bindgen_named_default_export() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("aria_web.js"),
            "export class WebRuntime {} export class WebPak {} export { __wbg_init as default }; aria_web_bg.wasm",
        )
        .unwrap();
        fs::write(
            temp.path().join("aria_web_bg.wasm"),
            [0, 0x61, 0x73, 0x6d, 1, 0, 0, 0],
        )
        .unwrap();
        validate_web_runtime_package(temp.path()).unwrap();
    }
}
