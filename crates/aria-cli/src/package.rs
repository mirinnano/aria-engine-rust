//! `aria package` — turns a built `aria build` output directory into public
//! distribution artifacts.
//!
//! The command keeps a deterministic zip as the portable interchange format,
//! then adds a native installer when the target has one: NSIS on Windows, a
//! self-extracting `.run` (and, when available, `.deb`/AppImage) on Linux, and
//! a macOS `.app` (and, when available, `.dmg`). Web builds also receive a
//! cache/header contract and a release manifest. Signing and notarization are
//! deliberately left to the platform CI, never hidden in this packager.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::build::{BuildManifest, BuildTarget};

/// Output policy for `aria package`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum PackageFormat {
    /// Portable archive plus every native installer available on this host.
    Auto,
    /// Only the deterministic portable zip.
    Zip,
    /// Only target-native installers. Fails when the required tool is absent.
    Installer,
    /// Web release archive plus static-host cache metadata.
    Web,
}

impl Default for PackageFormat {
    fn default() -> Self {
        Self::Auto
    }
}

// ── Minimal zip writer ──────────────────────────────────────────────
// The `zip` crate is not a workspace dependency; this implements the
// subset we need: DEFLATE + STORE, fixed timestamps, sorted entries.

/// Fixed DOS timestamp (2024-01-01 00:00:00), stored as the two u16 fields
/// required by both local and central ZIP headers.
const FIXED_DOS_TIME: u16 = 0;
const FIXED_DOS_DATE: u16 = ((2024 - 1980) << 9) | (1 << 5) | 1;

const SIG_LOCAL: u32 = 0x04034b50;
const SIG_CENTRAL: u32 = 0x02014b50;
const SIG_EOCD: u32 = 0x06054b50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompressionMethod {
    Store = 0,
    Deflate = 8,
}

struct ZipEntry {
    name: String,
    compressed_size: u32,
    uncompressed_size: u32,
    crc32: u32,
    method: CompressionMethod,
    local_header_offset: u32,
}

struct ZipWriter {
    buf: Vec<u8>,
    entries: Vec<ZipEntry>,
}

impl ZipWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            entries: Vec::new(),
        }
    }

    fn add_file(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let name_bytes = name.as_bytes();
        let crc = crc32_fast(data);
        let uncompressed_size = u32::try_from(data.len()).unwrap_or(u32::MAX);

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        let compressed = encoder.finish()?;

        let (method, compressed_data) = if compressed.len() < data.len() {
            (CompressionMethod::Deflate, compressed)
        } else {
            (CompressionMethod::Store, data.to_vec())
        };

        let local_header_offset = u32::try_from(self.buf.len()).unwrap_or(u32::MAX);

        let name_len = u16::try_from(name_bytes.len()).unwrap_or(u16::MAX);
        self.buf.extend_from_slice(&SIG_LOCAL.to_le_bytes());
        self.buf.extend_from_slice(&20u16.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes());
        self.buf.extend_from_slice(&(method as u16).to_le_bytes());
        self.buf.extend_from_slice(&FIXED_DOS_TIME.to_le_bytes());
        self.buf.extend_from_slice(&FIXED_DOS_DATE.to_le_bytes());
        self.buf.extend_from_slice(&crc.to_le_bytes());
        self.buf.extend_from_slice(
            &u32::try_from(compressed_data.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        self.buf.extend_from_slice(&uncompressed_size.to_le_bytes());
        self.buf.extend_from_slice(&name_len.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes());
        self.buf.extend_from_slice(name_bytes);
        self.buf.extend_from_slice(&compressed_data);

        self.entries.push(ZipEntry {
            name: name.to_owned(),
            compressed_size: u32::try_from(compressed_data.len()).unwrap_or(u32::MAX),
            uncompressed_size,
            crc32: crc,
            method,
            local_header_offset,
        });

        Ok(())
    }

    fn finish(mut self) -> Vec<u8> {
        let central_dir_offset = u32::try_from(self.buf.len()).unwrap_or(u32::MAX);

        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            let name_len = u16::try_from(name_bytes.len()).unwrap_or(u16::MAX);
            self.buf.extend_from_slice(&SIG_CENTRAL.to_le_bytes());
            self.buf.extend_from_slice(&20u16.to_le_bytes());
            self.buf.extend_from_slice(&20u16.to_le_bytes());
            self.buf.extend_from_slice(&0u16.to_le_bytes());
            self.buf
                .extend_from_slice(&(entry.method as u16).to_le_bytes());
            self.buf.extend_from_slice(&FIXED_DOS_TIME.to_le_bytes());
            self.buf.extend_from_slice(&FIXED_DOS_DATE.to_le_bytes());
            self.buf.extend_from_slice(&entry.crc32.to_le_bytes());
            self.buf
                .extend_from_slice(&entry.compressed_size.to_le_bytes());
            self.buf
                .extend_from_slice(&entry.uncompressed_size.to_le_bytes());
            self.buf.extend_from_slice(&name_len.to_le_bytes());
            self.buf.extend_from_slice(&0u16.to_le_bytes());
            self.buf.extend_from_slice(&0u16.to_le_bytes());
            self.buf.extend_from_slice(&0u16.to_le_bytes());
            self.buf.extend_from_slice(&0u16.to_le_bytes());
            self.buf.extend_from_slice(&0u32.to_le_bytes());
            self.buf
                .extend_from_slice(&entry.local_header_offset.to_le_bytes());
            self.buf.extend_from_slice(name_bytes);
        }

        let central_dir_size =
            u32::try_from(self.buf.len()).unwrap_or(u32::MAX) - central_dir_offset;
        let entry_count = u16::try_from(self.entries.len()).unwrap_or(u16::MAX);

        self.buf.extend_from_slice(&SIG_EOCD.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes());
        self.buf.extend_from_slice(&entry_count.to_le_bytes());
        self.buf.extend_from_slice(&entry_count.to_le_bytes());
        self.buf.extend_from_slice(&central_dir_size.to_le_bytes());
        self.buf
            .extend_from_slice(&central_dir_offset.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes());

        self.buf
    }
}

/// Fast CRC32 implementation (pure Rust, table-based).
fn crc32_fast(data: &[u8]) -> u32 {
    const TABLE_SIZE: usize = 256;
    static TABLE: LazyLock<[u32; TABLE_SIZE]> = LazyLock::new(|| {
        let mut t = [0u32; TABLE_SIZE];
        for (index, slot) in t.iter_mut().enumerate() {
            let mut crc = index as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    0xEDB88320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
            *slot = crc;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

// ── Bundle reading ──────────────────────────────────────────────────

fn read_build_manifest(bundle: &Path) -> Result<(BuildManifest, serde_json::Value)> {
    let manifest_path = bundle.join("build-manifest.json");
    let bytes = fs::read(&manifest_path).with_context(|| {
        format!(
            "bundle directory missing build-manifest.json: {}",
            bundle.display()
        )
    })?;
    let manifest: BuildManifest =
        serde_json::from_slice(&bytes).context("invalid build-manifest.json")?;
    let bundle_json: serde_json::Value = serde_json::from_slice(
        &fs::read(bundle.join("bundle.aria.json")).context("missing bundle.aria.json")?,
    )
    .context("invalid bundle.aria.json")?;
    Ok((manifest, bundle_json))
}

fn collect_entries(root: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .context("strip prefix from bundle entry")?
                .to_owned();
            entries.push(rel);
        }
    }
    entries.sort();
    Ok(entries)
}

fn create_deterministic_zip(bundle: &Path, output: &Path) -> Result<u64> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create output directory: {}", parent.display()))?;
    }

    let entries = collect_entries(bundle)?;
    if entries.is_empty() {
        bail!("bundle directory contains no files: {}", bundle.display());
    }

    let mut zip = ZipWriter::new();
    let mut buffer = Vec::new();

    for rel in &entries {
        let full = bundle.join(rel);
        let name = rel
            .to_str()
            .with_context(|| format!("non-UTF-8 path in bundle: {}", rel.display()))?;
        let name = name.replace('\\', "/");

        buffer.clear();
        fs::File::open(&full)
            .with_context(|| format!("cannot open {}", full.display()))?
            .read_to_end(&mut buffer)?;

        zip.add_file(&name, &buffer)
            .with_context(|| format!("cannot add {name} to zip"))?;
    }

    let bytes = zip.finish();
    let len = bytes.len() as u64;
    fs::write(output, &bytes)
        .with_context(|| format!("cannot write zip to {}", output.display()))?;
    Ok(len)
}

// ── Deterministic tar.gz used by Unix installers ────────────────────

/// Writes a small ustar archive without adding another runtime dependency to
/// the CLI. Keeping the writer here also lets the self-extracting Linux
/// installer preserve the executable bit of `aria-player`.
fn create_deterministic_tar_gz(root: &Path, output: &Path) -> Result<u64> {
    let entries = collect_entries(root)?;
    if entries.is_empty() {
        bail!("bundle directory contains no files: {}", root.display());
    }

    let mut tar = Vec::new();
    let mut buffer = Vec::new();
    for rel in entries {
        let full = root.join(&rel);
        let name = rel
            .to_str()
            .with_context(|| format!("non-UTF-8 path in bundle: {}", rel.display()))?
            .replace('\\', "/");
        append_tar_entry(&mut tar, &full, &name, &mut buffer)?;
    }

    // POSIX tar readers require two complete zero blocks at EOF.
    tar.resize(tar.len() + 1024, 0);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&tar)?;
    let bytes = encoder.finish()?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create output directory: {}", parent.display()))?;
    }
    fs::write(output, &bytes)
        .with_context(|| format!("cannot write tarball to {}", output.display()))?;
    Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

fn create_deterministic_tar_gz_for_app(app: &Path, output: &Path) -> Result<u64> {
    let entries = collect_entries(app)?;
    if entries.is_empty() {
        bail!("macOS app bundle contains no files: {}", app.display());
    }
    let app_name = app
        .file_name()
        .and_then(|name| name.to_str())
        .context("macOS app bundle has a non-UTF-8 name")?;
    let mut tar = Vec::new();
    let mut buffer = Vec::new();
    for rel in entries {
        let full = app.join(&rel);
        let relative = rel
            .to_str()
            .with_context(|| format!("non-UTF-8 path in app bundle: {}", rel.display()))?;
        let name = format!("{app_name}/{relative}").replace('\\', "/");
        append_tar_entry(&mut tar, &full, &name, &mut buffer)?;
    }
    tar.resize(tar.len() + 1024, 0);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&tar)?;
    let bytes = encoder.finish()?;
    if let Some(output_parent) = output.parent() {
        fs::create_dir_all(output_parent)?;
    }
    fs::write(output, &bytes)
        .with_context(|| format!("cannot write app archive {}", output.display()))?;
    Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

fn append_tar_entry(
    tar: &mut Vec<u8>,
    full: &Path,
    name: &str,
    buffer: &mut Vec<u8>,
) -> Result<()> {
    buffer.clear();
    fs::File::open(full)
        .with_context(|| format!("cannot open {}", full.display()))?
        .read_to_end(buffer)?;
    append_tar_file(tar, name, buffer, tar_mode(full))
}

fn append_tar_file(tar: &mut Vec<u8>, name: &str, bytes: &[u8], mode: u32) -> Result<()> {
    let name_bytes = name.as_bytes();
    let (file_name, prefix) = if name_bytes.len() <= 100 {
        (name, "")
    } else if let Some(index) = name.rfind('/') {
        let (prefix, file_name) = name.split_at(index);
        let file_name = &file_name[1..];
        if file_name.len() <= 100 && prefix.len() <= 155 {
            (file_name, prefix)
        } else {
            bail!("path is too long for ustar: {name}");
        }
    } else {
        bail!("path is too long for ustar: {name}");
    };

    let mut header = [0u8; 512];
    copy_tar_field(&mut header[0..100], file_name.as_bytes());
    write_tar_octal(&mut header[100..108], u64::from(mode));
    write_tar_octal(&mut header[108..116], 0);
    write_tar_octal(&mut header[116..124], 0);
    write_tar_octal(
        &mut header[124..136],
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    );
    write_tar_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    copy_tar_field(&mut header[265..297], b"root");
    copy_tar_field(&mut header[297..329], b"root");
    copy_tar_field(&mut header[345..500], prefix.as_bytes());

    let checksum = header.iter().map(|byte| u32::from(*byte)).sum::<u32>();
    let checksum_text = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(checksum_text.as_bytes());
    tar.extend_from_slice(&header);
    tar.extend_from_slice(bytes);
    let padding = (512 - bytes.len() % 512) % 512;
    tar.resize(tar.len() + padding, 0);
    Ok(())
}

fn copy_tar_field(field: &mut [u8], value: &[u8]) {
    let length = value.len().min(field.len());
    field[..length].copy_from_slice(&value[..length]);
}

fn write_tar_octal(field: &mut [u8], value: u64) {
    field.fill(0);
    let width = field.len().saturating_sub(1);
    let text = format!("{value:0width$o}", width = width);
    let bytes = text.as_bytes();
    let start = bytes.len().saturating_sub(width);
    let copy_start = width.saturating_sub(bytes.len() - start);
    field[copy_start..copy_start + bytes.len() - start].copy_from_slice(&bytes[start..]);
    field[field.len() - 1] = 0;
}

fn tar_mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map(|metadata| metadata.permissions().mode())
            .unwrap_or(0o644);
        if mode & 0o111 != 0 { 0o755 } else { 0o644 }
    }
    #[cfg(not(unix))]
    {
        if path.file_name().is_some_and(|name| name == "aria-player") {
            0o755
        } else {
            0o644
        }
    }
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(not(unix))]
    let _ = path;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        if entry.path() == source {
            continue;
        }
        if entry.file_type().is_symlink() {
            bail!(
                "installer input contains a symbolic link: {}",
                entry.path().display()
            );
        }
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &target).with_context(|| {
            format!(
                "cannot copy installer input {} to {}",
                entry.path().display(),
                target.display()
            )
        })?;
        #[cfg(unix)]
        fs::set_permissions(&target, fs::metadata(entry.path())?.permissions())?;
    }
    Ok(())
}

fn tool_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn safe_stem(value: &str) -> String {
    let mut stem = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    let stem = stem.trim_matches('-').to_owned();
    if stem.is_empty() {
        "aria-game".to_owned()
    } else {
        stem
    }
}

fn debian_version(value: &str) -> String {
    let mut version = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '~' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if !version
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        version.insert(0, '0');
    }
    version
}

// ── Linux installers ───────────────────────────────────────────────

fn generate_linux_run_installer(
    bundle: &Path,
    game_id: &str,
    game_version: &str,
    game_title: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    let stem = safe_stem(game_id);
    let version_stem = safe_stem(game_version);
    let tarball = output_dir.join(format!("{stem}-{version_stem}-linux-x64.tar.gz"));
    create_deterministic_tar_gz(bundle, &tarball)?;
    let payload = fs::read(&tarball)?;
    let desktop_id = stem.replace('.', "-");
    let title = shell_literal(game_title);
    let mut script = format!(
        "#!/bin/sh\n\nset -eu\n\npayload_offset=00000000000000000000\ngame_id='{stem}'\ngame_title='{title}'\n\nif [ \"${{1:-}}\" = \"--help\" ]; then\n  printf '%s\\n' \"Usage: $0 [--install-dir DIR]\"\n  exit 0\nfi\n\ninstall_dir=\"${{HOME}}/.local/share/${{game_id}}\"\nif [ \"${{1:-}}\" = \"--install-dir\" ]; then\n  [ \"${{2:-}}\" ] || {{ echo 'missing install directory' >&2; exit 2; }}\n  install_dir=\"$2\"\nfi\n\nstaging=\"$(mktemp -d 2>/dev/null || mktemp -d -t aria-install)\"\ncleanup() {{ rm -rf \"$staging\"; }}\ntrap cleanup EXIT INT TERM\nmkdir -p \"$install_dir\"\ntail -c +\"$payload_offset\" \"$0\" | tar -xzf - -C \"$staging\"\ncp -R \"$staging\"/. \"$install_dir\"/\nchmod +x \"$install_dir/aria-player\"\nmkdir -p \"${{XDG_DATA_HOME:-$HOME/.local/share}}/applications\"\ncat > \"${{XDG_DATA_HOME:-$HOME/.local/share}}/applications/{desktop_id}.desktop\" <<EOF\n[Desktop Entry]\nType=Application\nName=$game_title\nExec=$install_dir/aria-player\nPath=$install_dir\nTerminal=false\nCategories=Game;\nEOF\nprintf 'Installed %s to %s\\n' \"$game_title\" \"$install_dir\"\nexit 0\n\n__ARIA_PAYLOAD_BELOW__\n"
    );
    let payload_offset = script.len() + 1;
    let offset_text = format!("{payload_offset:020}");
    script = script.replace("00000000000000000000", &offset_text);
    let mut bytes = script.into_bytes();
    bytes.extend_from_slice(&payload);
    let output = output_dir.join(format!("{stem}-{version_stem}-linux-x64.run"));
    fs::write(&output, bytes)
        .with_context(|| format!("cannot write Linux installer {}", output.display()))?;
    set_executable(&output)?;
    Ok(output)
}

fn shell_literal(value: &str) -> String {
    value.replace('\'', "'\\''").replace('\n', " ")
}

fn xml_literal(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', " ")
}

fn generate_linux_deb(
    bundle: &Path,
    game_id: &str,
    game_version: &str,
    game_title: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    let dpkg = tool_on_path("dpkg-deb").context(
        "dpkg-deb is not available; install dpkg-dev or use the generated .run installer",
    )?;
    let stem = safe_stem(game_id);
    let version_stem = safe_stem(game_version);
    let stage = output_dir.join(format!(".{stem}-deb-stage"));
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    let payload_root = stage.join("usr/lib").join(&stem);
    fs::create_dir_all(&payload_root)?;
    copy_tree(bundle, &payload_root)?;
    if payload_root.join("aria-player").is_file() {
        set_executable(&payload_root.join("aria-player"))?;
    }
    let desktop_root = stage.join("usr/share/applications");
    fs::create_dir_all(&desktop_root)?;
    fs::write(
        desktop_root.join(format!("{stem}.desktop")),
        format!(
            "[Desktop Entry]\nType=Application\nName={game_title}\nExec=/usr/lib/{stem}/aria-player\nPath=/usr/lib/{stem}\nTerminal=false\nCategories=Game;\n"
        ),
    )?;
    let control = stage.join("DEBIAN");
    fs::create_dir_all(&control)?;
    fs::write(
        control.join("control"),
        format!(
            "Package: {stem}\nVersion: {}\nSection: games\nPriority: optional\nArchitecture: amd64\nMaintainer: Aria Engine <release@aria.example>\nDescription: {game_title}\n Portable Aria visual novel package.\n",
            debian_version(game_version)
        ),
    )?;
    let output = output_dir.join(format!("{stem}-{version_stem}-linux-x64.deb"));
    let status = Command::new(dpkg)
        .args(["--build", "--root-owner-group"])
        .arg(&stage)
        .arg(&output)
        .status()
        .context("failed to start dpkg-deb")?;
    let _ = fs::remove_dir_all(&stage);
    if !status.success() {
        bail!("dpkg-deb failed with status {status}");
    }
    if !output.is_file() {
        bail!("dpkg-deb did not produce {}", output.display());
    }
    Ok(output)
}

fn generate_linux_appimage(
    bundle: &Path,
    game_id: &str,
    game_version: &str,
    game_title: &str,
    output_dir: &Path,
) -> Result<Option<PathBuf>> {
    let Some(appimagetool) = tool_on_path("appimagetool") else {
        return Ok(None);
    };
    let stem = safe_stem(game_id);
    let version_stem = safe_stem(game_version);
    let stage = output_dir.join(format!(".{stem}-AppDir"));
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    let payload_root = stage.join("usr/lib").join(&stem);
    fs::create_dir_all(&payload_root)?;
    copy_tree(bundle, &payload_root)?;
    set_executable(&payload_root.join("aria-player"))?;
    fs::write(
        stage.join("AppRun"),
        format!(
            "#!/bin/sh\nset -eu\nhere=\"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\nexec \"$here/usr/lib/{stem}/aria-player\" \"$@\"\n"
        ),
    )?;
    set_executable(&stage.join("AppRun"))?;
    fs::write(
        stage.join(format!("{stem}.desktop")),
        format!(
            "[Desktop Entry]\nType=Application\nName={game_title}\nExec=aria-player\nTerminal=false\nCategories=Game;\n"
        ),
    )?;
    let output = output_dir.join(format!("{stem}-{version_stem}-linux-x64.AppImage"));
    let status = Command::new(appimagetool)
        .arg(&stage)
        .arg(&output)
        .status()
        .context("failed to start appimagetool")?;
    let _ = fs::remove_dir_all(&stage);
    if !status.success() {
        bail!("appimagetool failed with status {status}");
    }
    if !output.is_file() {
        bail!("appimagetool did not produce {}", output.display());
    }
    Ok(Some(output))
}

fn generate_linux_installers(
    bundle: &Path,
    game_id: &str,
    game_version: &str,
    game_title: &str,
    output_dir: &Path,
    strict: bool,
) -> Result<Vec<PathBuf>> {
    let mut artifacts = Vec::new();
    let run = generate_linux_run_installer(bundle, game_id, game_version, game_title, output_dir)?;
    artifacts.push(run);
    match generate_linux_deb(bundle, game_id, game_version, game_title, output_dir) {
        Ok(path) => artifacts.push(path),
        Err(error) if strict => return Err(error),
        Err(error) => eprintln!("  Note: Debian installer skipped: {error}"),
    }
    match generate_linux_appimage(bundle, game_id, game_version, game_title, output_dir) {
        Ok(Some(path)) => artifacts.push(path),
        Ok(None) => eprintln!("  Note: AppImage skipped: appimagetool is not on PATH"),
        Err(error) if strict => return Err(error),
        Err(error) => eprintln!("  Note: AppImage skipped: {error}"),
    }
    Ok(artifacts)
}

// ── macOS installers ───────────────────────────────────────────────

fn generate_macos_app(
    bundle: &Path,
    game_id: &str,
    game_version: &str,
    game_title: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    let stem = safe_stem(game_id);
    let app = output_dir.join(format!("{stem}.app"));
    if app.exists() {
        fs::remove_dir_all(&app)?;
    }
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    let player = bundle.join("aria-player");
    if !player.is_file() {
        bail!("macOS bundle is missing aria-player: {}", player.display());
    }
    fs::copy(&player, macos.join("aria-player"))?;
    set_executable(&macos.join("aria-player"))?;
    for entry in walkdir::WalkDir::new(bundle).follow_links(false) {
        let entry = entry?;
        if entry.path() == bundle || entry.path() == player {
            continue;
        }
        let relative = entry.path().strip_prefix(bundle)?;
        let target = resources.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    fs::write(
        contents.join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"https://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleDisplayName</key><string>{game_title}</string><key>CFBundleExecutable</key><string>aria-player</string><key>CFBundleIdentifier</key><string>{stem}</string><key>CFBundleName</key><string>{game_title}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>{game_version}</string><key>CFBundleVersion</key><string>{game_version}</string><key>LSMinimumSystemVersion</key><string>11.0</string></dict></plist>\n",
            game_title = xml_literal(game_title),
            game_version = xml_literal(game_version),
        ),
    )?;
    Ok(app)
}

fn generate_macos_installers(
    bundle: &Path,
    game_id: &str,
    game_version: &str,
    game_title: &str,
    output_dir: &Path,
    strict: bool,
) -> Result<Vec<PathBuf>> {
    let app = generate_macos_app(bundle, game_id, game_version, game_title, output_dir)?;
    let mut artifacts = Vec::new();
    let version_stem = safe_stem(game_version);
    let app_archive = output_dir.join(format!(
        "{}-{version_stem}-macos-universal.app.tar.gz",
        safe_stem(game_id)
    ));
    create_deterministic_tar_gz_for_app(&app, &app_archive)?;
    artifacts.push(app_archive);

    if let Some(hdiutil) = tool_on_path("hdiutil") {
        let dmg = output_dir.join(format!(
            "{}-{version_stem}-macos-universal.dmg",
            safe_stem(game_id)
        ));
        let status = Command::new(hdiutil)
            .args(["create", "-volname", game_title, "-srcfolder"])
            .arg(&app)
            .args(["-ov", "-format", "UDZO"])
            .arg(&dmg)
            .status()
            .context("failed to start hdiutil")?;
        if !status.success() {
            if strict {
                bail!("hdiutil failed with status {status}");
            }
            eprintln!("  Note: DMG skipped: hdiutil failed with status {status}");
        } else if dmg.is_file() {
            artifacts.push(dmg);
        }
    } else if strict {
        bail!("hdiutil is not available; run the macOS installer step on macOS");
    } else {
        eprintln!("  Note: DMG skipped: hdiutil is not on PATH");
    }
    Ok(artifacts)
}

// ── Web/release metadata ───────────────────────────────────────────

fn write_web_release_contract(
    bundle: &Path,
    output_dir: &Path,
    game_id: &str,
    game_version: &str,
    bundle_json: &serde_json::Value,
) -> Result<Vec<PathBuf>> {
    let mut immutable = vec![
        "pkg/aria_web.js".to_owned(),
        "pkg/aria_web_bg.wasm".to_owned(),
        "web-renderer.js".to_owned(),
        "web-audio.js".to_owned(),
        "save-store.js".to_owned(),
    ];
    if let Some(packs) = bundle_json
        .get("pak_packs")
        .and_then(|value| value.as_array())
    {
        immutable.extend(
            packs
                .iter()
                .filter_map(|pack| pack.get("file").and_then(|value| value.as_str()))
                .map(str::to_owned),
        );
    } else {
        immutable.push("game.ariapak".to_owned());
    }
    immutable.sort();
    immutable.dedup();
    let contract = json!({
        "schema_version": 1,
        "game_id": game_id,
        "game_version": game_version,
        "mode": "static-pwa",
        "entrypoints": ["index.html", "manifest.webmanifest", "service-worker.js"],
        "immutable_files": immutable,
        "cache_policy": {
            "immutable": "public, max-age=31536000, immutable",
            "entrypoint": "no-cache"
        }
    });
    let site = output_dir.join("site");
    if site.exists() {
        fs::remove_dir_all(&site)?;
    }
    fs::create_dir_all(&site)?;
    copy_tree(bundle, &site)?;
    let contract_bytes = serde_json::to_vec_pretty(&contract)?;
    let headers = "/pkg/*\n  Cache-Control: public, max-age=31536000, immutable\n/game*.ariapak\n  Cache-Control: public, max-age=31536000, immutable\n/index.html\n  Cache-Control: no-cache\n/manifest.webmanifest\n  Cache-Control: no-cache\n/service-worker.js\n  Cache-Control: no-cache\n";
    let contract_path = output_dir.join("web-release.json");
    fs::write(&contract_path, &contract_bytes)?;
    fs::write(site.join("web-release.json"), &contract_bytes)?;
    let headers_path = output_dir.join("_headers");
    fs::write(&headers_path, headers)?;
    fs::write(site.join("_headers"), headers)?;
    Ok(vec![contract_path, headers_path])
}

fn write_release_metadata(
    output_dir: &Path,
    game_id: &str,
    game_version: &str,
    target: &str,
    bundle: &serde_json::Value,
    artifacts: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for artifact in artifacts {
        if !artifact.is_file() {
            continue;
        }
        let bytes = fs::read(artifact)?;
        let digest = Sha256::digest(&bytes);
        let name = artifact
            .strip_prefix(output_dir)
            .unwrap_or(artifact)
            .to_string_lossy()
            .replace('\\', "/");
        files.push(json!({
            "name": name,
            "size": bytes.len(),
            "sha256": format!("{digest:x}")
        }));
    }
    files.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    let manifest = json!({
        "schema_version": 1,
        "game_id": game_id,
        "game_version": game_version,
        "target": target,
        "bundle_content_root_blake3": bundle.get("content_root_blake3"),
        "pak_profile": bundle.get("pak_profile"),
        "artifacts": files
    });
    let manifest_path = output_dir.join("release-manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    let mut checksums = String::new();
    for file in &files {
        checksums.push_str(file["sha256"].as_str().unwrap_or_default());
        checksums.push_str("  ");
        checksums.push_str(file["name"].as_str().unwrap_or_default());
        checksums.push('\n');
    }
    let checksums_path = output_dir.join("checksums.sha256");
    fs::write(&checksums_path, checksums)?;
    Ok(vec![manifest_path, checksums_path])
}

// ── NSIS installer generation ───────────────────────────────────────

fn generate_nsis_installer(
    bundle: &Path,
    manifest: &BuildManifest,
    bundle_json: &serde_json::Value,
    output_dir: &Path,
) -> Result<()> {
    let repo_root = find_repo_root(bundle)?;
    let template = repo_root.join("installer/aria-game.nss");
    if !template.is_file() {
        bail!(
            "NSIS template not found at {}; cannot generate installer",
            template.display()
        );
    }

    let game_id = bundle_json["game_id"].as_str().unwrap_or("unknown");
    let game_version = bundle_json["game_version"].as_str().unwrap_or("dev");
    let game_title = bundle_json["game_title"].as_str().unwrap_or("Aria Game");
    let publisher = bundle_json
        .get("publisher")
        .and_then(|v| v.as_str())
        .unwrap_or("Ponkotusoft");

    let player_filename = match manifest.target {
        BuildTarget::WindowsX64 => "aria-player.exe",
        _ => "aria-player",
    };

    fs::create_dir_all(output_dir)?;
    let nsi_path = output_dir.join("installer.nsi");
    let mut script = fs::read_to_string(&template)
        .with_context(|| format!("cannot read NSIS template: {}", template.display()))?;

    let appdir = bundle.display().to_string().replace('/', "\\");
    let outfile = output_dir
        .join(format!("{game_id}-setup.exe"))
        .display()
        .to_string()
        .replace('/', "\\");

    script = script.replace("{{APPDIR}}", &appdir);
    script = script.replace("{{OUTFILE}}", &outfile);
    script = script.replace("{{VERSION}}", game_version);
    script = script.replace("{{PRODUCT_NAME}}", game_title);
    script = script.replace("{{PUBLISHER}}", publisher);
    script = script.replace("{{PLAYER_FILENAME}}", player_filename);

    fs::write(&nsi_path, &script)?;

    let status = std::process::Command::new("makensis")
        .arg(&nsi_path)
        .status()
        .context("makensis not found on PATH; install NSIS to generate a Windows installer")?;
    if !status.success() {
        bail!("makensis exited with status {status}");
    }

    let installer_path = output_dir.join(format!("{game_id}-setup.exe"));
    if !installer_path.is_file() {
        bail!("makensis did not produce the expected installer");
    }

    println!("  NSIS installer: {}", installer_path.display());
    Ok(())
}

fn find_repo_root(start: &Path) -> Result<PathBuf> {
    let mut current = start
        .canonicalize()
        .with_context(|| format!("cannot canonicalize: {}", start.display()))?;
    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.is_file()
            && let Ok(contents) = fs::read_to_string(&cargo_toml)
            && (contents.contains("aria-core") || contents.contains("aria-engine"))
        {
            return Ok(current);
        }
        if !current.pop() {
            break;
        }
    }
    bail!(
        "cannot locate repository root from {}; place installer/aria-game.nss in the repo root",
        start.display()
    );
}

// ── Command entry ───────────────────────────────────────────────────

pub fn command(bundle: &Path, out: Option<&Path>, format: PackageFormat) -> Result<u8> {
    let bundle = bundle
        .canonicalize()
        .with_context(|| format!("bundle directory not found: {}", bundle.display()))?;

    if !bundle.is_dir() {
        bail!("bundle path is not a directory: {}", bundle.display());
    }

    let (manifest, bundle_json) = read_build_manifest(&bundle)?;

    let game_id = bundle_json["game_id"]
        .as_str()
        .context("bundle.aria.json missing game_id")?;
    let game_version = bundle_json["game_version"]
        .as_str()
        .context("bundle.aria.json missing game_version")?;
    let game_title = bundle_json["game_title"].as_str().unwrap_or(game_id);
    let target_str = manifest.target.as_str();

    let output_dir = out
        .map(PathBuf::from)
        .unwrap_or_else(|| bundle.parent().expect("bundle has no parent").join("dist"));
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("cannot create output directory: {}", output_dir.display()))?;
    println!("Packaging bundle: {}", bundle.display());
    println!("  Target: {}", target_str);
    println!("  Game: {game_id} v{game_version}");

    if format == PackageFormat::Installer && manifest.target == BuildTarget::Web {
        bail!("web bundles use --format web or --format auto; there is no native installer");
    }
    if format == PackageFormat::Web && manifest.target != BuildTarget::Web {
        bail!("--format web is only valid for a web bundle");
    }

    let mut artifacts = Vec::new();
    let web_metadata = if manifest.target == BuildTarget::Web
        && matches!(format, PackageFormat::Auto | PackageFormat::Web)
    {
        Some(write_web_release_contract(
            &bundle,
            &output_dir,
            game_id,
            game_version,
            &bundle_json,
        )?)
    } else {
        None
    };
    if matches!(
        format,
        PackageFormat::Auto | PackageFormat::Zip | PackageFormat::Web
    ) {
        let zip_name = format!("{game_id}-{game_version}-{target_str}.zip");
        let zip_path = output_dir.join(&zip_name);
        let zip_root = if web_metadata.is_some() {
            output_dir.join("site")
        } else {
            bundle.clone()
        };
        let zip_size = create_deterministic_zip(&zip_root, &zip_path)?;
        println!(
            "  Zip: {} ({:.2} MB)",
            zip_path.display(),
            zip_size as f64 / 1_048_576.0
        );
        artifacts.push(zip_path);
    }

    if matches!(format, PackageFormat::Auto | PackageFormat::Installer)
        && manifest.target != BuildTarget::Web
    {
        let strict = format == PackageFormat::Installer;
        match manifest.target {
            BuildTarget::WindowsX64 => {
                match generate_nsis_installer(&bundle, &manifest, &bundle_json, &output_dir) {
                    Ok(()) => artifacts.push(output_dir.join(format!("{game_id}-setup.exe"))),
                    Err(error) if strict => return Err(error),
                    Err(error) => {
                        eprintln!("  Note: NSIS installer generation skipped: {error}");
                        eprintln!("  Install NSIS (makensis) to generate a Windows installer.");
                    }
                }
            }
            BuildTarget::LinuxX64 | BuildTarget::SteamdeckX64 => {
                artifacts.extend(generate_linux_installers(
                    &bundle,
                    game_id,
                    game_version,
                    game_title,
                    &output_dir,
                    strict,
                )?);
            }
            BuildTarget::MacosUniversal => {
                artifacts.extend(generate_macos_installers(
                    &bundle,
                    game_id,
                    game_version,
                    game_title,
                    &output_dir,
                    strict,
                )?);
            }
            BuildTarget::Web => unreachable!("web installer rejected above"),
        }
    }

    if let Some(web_metadata) = web_metadata.as_ref() {
        artifacts.extend(web_metadata.iter().cloned());
    }
    let metadata = write_release_metadata(
        &output_dir,
        game_id,
        game_version,
        target_str,
        &bundle_json,
        &artifacts,
    )?;
    println!("  Release manifest: {}", metadata[0].display());
    println!("  Checksums: {}", metadata[1].display());
    if manifest.target == BuildTarget::MacosUniversal {
        println!("  macOS signing/notarization remains a CI step (codesign + notarytool).");
    }
    if web_metadata.is_some() {
        println!(
            "  Web contract: {}",
            output_dir.join("web-release.json").display()
        );
    }
    println!("Packaged to {}", output_dir.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_zip_produces_identical_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("game.ariac"), b"ariac-data").unwrap();
        fs::write(bundle.join("bundle.aria.json"), r#"{"schema_version":5}"#).unwrap();

        let zip1 = temp.path().join("out1.zip");
        let zip2 = temp.path().join("out2.zip");
        create_deterministic_zip(&bundle, &zip1).unwrap();
        create_deterministic_zip(&bundle, &zip2).unwrap();

        assert_eq!(
            fs::read(&zip1).unwrap(),
            fs::read(&zip2).unwrap(),
            "deterministic zip outputs must be byte-identical"
        );
    }

    #[test]
    fn deterministic_zip_is_sorted() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("bundle");
        fs::create_dir_all(bundle.join("z-dir")).unwrap();
        fs::create_dir_all(bundle.join("a-dir")).unwrap();
        fs::write(bundle.join("z-dir/z.txt"), b"zzzzzzzzzz").unwrap();
        fs::write(bundle.join("a-dir/a.txt"), b"aaaaaaaaaa").unwrap();
        fs::write(bundle.join("m.txt"), b"mmmmmmmmmm").unwrap();

        let zip_path = temp.path().join("out.zip");
        create_deterministic_zip(&bundle, &zip_path).unwrap();

        let bytes = fs::read(&zip_path).unwrap();
        let mut names = Vec::new();
        let mut pos = 0;
        while pos + 4 <= bytes.len() {
            let sig = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            if sig == SIG_LOCAL {
                let name_len =
                    u16::from_le_bytes(bytes[pos + 26..pos + 28].try_into().unwrap()) as usize;
                let extra_len =
                    u16::from_le_bytes(bytes[pos + 28..pos + 30].try_into().unwrap()) as usize;
                let name =
                    String::from_utf8_lossy(&bytes[pos + 30..pos + 30 + name_len]).to_string();
                names.push(name);
                let comp_len =
                    u32::from_le_bytes(bytes[pos + 18..pos + 22].try_into().unwrap()) as usize;
                pos += 30 + name_len + extra_len + comp_len;
            } else if sig == SIG_CENTRAL || sig == SIG_EOCD {
                break;
            } else {
                pos += 1;
            }
        }
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "zip entries must be sorted");
    }

    #[test]
    fn crc32_fast_matches_known_vector() {
        assert_eq!(crc32_fast(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn deterministic_zip_headers_use_the_standard_two_byte_dos_fields() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("index.html"), b"hello").unwrap();
        let zip_path = temp.path().join("out.zip");
        create_deterministic_zip(&bundle, &zip_path).unwrap();
        let bytes = fs::read(zip_path).unwrap();
        assert_eq!(&bytes[0..4], &SIG_LOCAL.to_le_bytes());
        let compressed =
            u32::from_le_bytes(bytes[18..22].try_into().expect("compressed size field"));
        let uncompressed =
            u32::from_le_bytes(bytes[22..26].try_into().expect("uncompressed size field"));
        let name_length =
            u16::from_le_bytes(bytes[26..28].try_into().expect("name length field")) as usize;
        assert_eq!(uncompressed, 5);
        assert!(compressed > 0);
        assert_eq!(&bytes[30..30 + name_length], b"index.html");
        assert!(
            bytes
                .windows(4)
                .any(|window| window == SIG_CENTRAL.to_le_bytes())
        );
    }
}
