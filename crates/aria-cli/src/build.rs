use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aria_core::Severity;
use aria_core::pak::AssetInput;
use aria_protection::{
    LicensePolicy, PakBuildInput, PakEncryptionKey, PakPackage, PakProfile, PakRole, PakSigningKey,
    StaticPakKeyProvider,
};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::package_runtime::{
    copy_native_player, copy_web_runtime, native_player_filename, resolve_native_player,
    resolve_web_runtime,
};
use crate::project::{AssetInventory, LoadedProject};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildTarget {
    #[value(name = "windows-x64")]
    WindowsX64,
    #[value(name = "macos-universal")]
    MacosUniversal,
    #[value(name = "linux-x64")]
    LinuxX64,
    #[value(name = "steamdeck-x64")]
    SteamdeckX64,
    #[value(name = "web")]
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildProfile {
    Dev,
    Signed,
    Protected,
}

impl Default for BuildProfile {
    fn default() -> Self {
        Self::Dev
    }
}

impl BuildProfile {
    #[must_use]
    pub const fn as_pak_profile(self) -> PakProfile {
        match self {
            Self::Dev => PakProfile::Dev,
            Self::Signed => PakProfile::Signed,
            Self::Protected => PakProfile::Protected,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PakBuildKeys {
    pub signing: Option<PakSigningKey>,
    pub encryption: Option<PakEncryptionKey>,
}

impl BuildTarget {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows-x64",
            Self::MacosUniversal => "macos-universal",
            Self::LinuxX64 => "linux-x64",
            Self::SteamdeckX64 => "steamdeck-x64",
            Self::Web => "web",
        }
    }
}

/// Target-independent game data. A release can wrap this same manifest,
/// bytecode, and pak in Windows, Linux, or Web Players without changing game
/// behaviour or asset identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub engine_version: String,
    pub language_major: u16,
    pub language_minor: u16,
    pub vm_abi_version: u16,
    pub game_id: String,
    pub game_version: String,
    pub game_title: String,
    pub logical_width: u32,
    pub logical_height: u32,
    pub save_namespace: String,
    /// Ordered logical paths to the only fonts a Player may use. The order is
    /// part of the portable bundle root because it defines fallback order.
    pub font_assets: Vec<String>,
    pub ariac_blake3: String,
    pub ariac_size: u64,
    pub pak_blake3: String,
    pub pak_size: u64,
    pub pak_content_root_blake3: String,
    pub pak_profile: BuildProfile,
    pub pack_id: String,
    pub content_root_blake3: String,
    pub integrity: String,
}

/// Target-specific wrapper metadata. It intentionally contains no game data;
/// `bundle.aria.json` is the authoritative portable bundle contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildManifest {
    pub schema_version: u32,
    pub engine_version: String,
    pub target: BuildTarget,
    pub bundle_blake3: String,
    pub player_binary_included: bool,
    pub web_runtime_package_required: bool,
}

pub fn command(path: &Path, target: BuildTarget, out: Option<&Path>, release: bool) -> Result<u8> {
    command_with_profile(path, target, out, release, BuildProfile::Dev, None, None)
}

pub fn command_with_profile(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    release: bool,
    profile: BuildProfile,
    signing_key: Option<&str>,
    encryption_key: Option<&str>,
) -> Result<u8> {
    let keys = resolve_pak_keys(profile, signing_key, encryption_key)?;
    let output = build_project_with_profile_and_keys(path, target, out, release, profile, keys)?;
    println!("built {}", output.display());
    Ok(0)
}

pub fn build_project(path: &Path, target: BuildTarget, out: Option<&Path>) -> Result<PathBuf> {
    build_project_with_release(path, target, out, false)
}

/// Builds a selected PAK profile. Keys are read from `ARIA_PAK_SIGNING_KEY`
/// and `ARIA_PAK_ENCRYPTION_KEY` when the profile needs them; callers that
/// already own key material should use the `_and_keys` variant.
pub fn build_project_with_profile(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    profile: BuildProfile,
) -> Result<PathBuf> {
    let keys = resolve_pak_keys(profile, None, None)?;
    build_project_with_profile_and_keys(path, target, out, false, profile, keys)
}

fn build_project_with_release(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    release: bool,
) -> Result<PathBuf> {
    build_project_with_profile_and_keys(
        path,
        target,
        out,
        release,
        BuildProfile::Dev,
        PakBuildKeys::default(),
    )
}

pub fn build_project_with_profile_and_keys(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    release: bool,
    profile: BuildProfile,
    pak_keys: PakBuildKeys,
) -> Result<PathBuf> {
    let project = LoadedProject::load(path)?;
    let compiled = project.compile()?;
    for diagnostic in &compiled.diagnostics {
        eprintln!("{diagnostic}");
    }
    if compiled.has_errors() {
        bail!("project has compiler errors");
    }
    let unsupported_count = crate::release::unsupported_runtime_command_count(&compiled);
    if release && unsupported_count > 0 {
        bail!(
            "release build rejected {unsupported_count} unsupported runtime command(s); migrate or implement them before packaging"
        );
    }
    if release && !crate::release::has_release_language(&compiled) {
        bail!(
            "release build requires structured 'aria 3.1;' source; run 'aria migrate' before packaging"
        );
    }
    if release && profile == BuildProfile::Dev {
        bail!(
            "release build requires an explicit signed or protected PAK profile; use --profile signed or --profile protected"
        );
    }
    let warning_count = compiled
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    if warning_count > 0 {
        eprintln!(
            "warning: build contains {warning_count} vertical-slice compatibility warning(s)"
        );
    }
    let program = compiled.program.context("compiler produced no program")?;
    let ariac = program.encode()?;
    let asset_inventory = project.asset_inventory()?;
    project.validate_bundled_fonts(&asset_inventory, release)?;
    let assets = collect_assets(&asset_inventory)?;

    let ariac_blake3 = blake3::hash(&ariac).to_hex().to_string();
    let pack_id = format!("{}.boot", project.manifest.game.id);
    let (pak, pak_content_root) = build_pak(
        profile,
        &project.manifest.game.id,
        &pack_id,
        assets,
        pak_keys,
    )?;
    let pak_blake3 = blake3::hash(&pak).to_hex().to_string();
    let mut bundle = BundleManifest {
        schema_version: 5,
        engine_version: aria_core::ENGINE_VERSION.to_owned(),
        language_major: program.language_version.major,
        language_minor: program.language_version.minor,
        vm_abi_version: aria_core::bytecode::ARIAC_VM_ABI_VERSION,
        game_id: project.manifest.game.id.clone(),
        game_version: project.manifest.game.version.clone(),
        game_title: project.manifest.game.title.clone(),
        logical_width: project.manifest.runtime.logical_width,
        logical_height: project.manifest.runtime.logical_height,
        save_namespace: project.manifest.runtime.save_namespace.clone(),
        font_assets: project.manifest.runtime.fonts.clone(),
        ariac_blake3,
        ariac_size: u64::try_from(ariac.len()).context("compiled program is too large")?,
        pak_blake3,
        pak_size: u64::try_from(pak.len()).context("asset pak is too large")?,
        pak_content_root_blake3: pak_content_root,
        pak_profile: profile,
        pack_id,
        content_root_blake3: String::new(),
        integrity: match profile {
            BuildProfile::Dev => {
                "BLAKE3 corruption detection only; unsigned development bundle".to_owned()
            }
            BuildProfile::Signed => {
                "BLAKE3 chunk hashes plus Ed25519 publisher signature".to_owned()
            }
            BuildProfile::Protected => {
                "BLAKE3 chunk hashes plus Ed25519 signature and XChaCha20-Poly1305 chunk encryption"
                    .to_owned()
            }
        },
    };
    bundle.content_root_blake3 = bundle_content_root(&bundle);
    let bundle_bytes = serde_json::to_vec_pretty(&bundle)?;

    // Resolve target executables and browser glue before creating any staging
    // directory. A failed release preflight must not leave a half-written
    // package that a later invocation could mistake for a valid artifact.
    let native_player = resolve_native_player(target, release)?;
    let web_runtime = if target == BuildTarget::Web {
        resolve_web_runtime(release)?
    } else {
        None
    };

    let destination = out
        .map(Path::to_owned)
        .unwrap_or_else(|| project.root.join("dist").join(target.as_str()));
    let parent = destination
        .parent()
        .context("build output has no parent directory")?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".aria-build-{}-{}",
        target.as_str(),
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    fs::write(staging.join("game.ariac"), &ariac)?;
    fs::write(staging.join("game.ariapak"), &pak)?;
    fs::write(staging.join("bundle.aria.json"), &bundle_bytes)?;

    let player_binary_included = if let Some(player) = native_player {
        copy_native_player(&player, &staging.join(native_player_filename(target)))?;
        true
    } else {
        false
    };
    let web_runtime_included = if target == BuildTarget::Web {
        write_pwa_shell(&staging, web_runtime.as_deref())?
    } else {
        false
    };

    let manifest = BuildManifest {
        schema_version: 5,
        engine_version: aria_core::ENGINE_VERSION.to_owned(),
        target,
        bundle_blake3: blake3::hash(&bundle_bytes).to_hex().to_string(),
        player_binary_included,
        web_runtime_package_required: target == BuildTarget::Web && !web_runtime_included,
    };
    fs::write(
        staging.join("build-manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    if destination.exists() {
        fs::remove_dir_all(&destination)
            .with_context(|| format!("cannot replace {}", destination.display()))?;
    }
    fs::rename(&staging, &destination)?;
    Ok(destination)
}

/// Stable identity for all data that influences Core execution. It is kept
/// separate from the target wrapper so a Windows and Linux build can prove
/// that they carry exactly the same game bundle.
#[must_use]
pub fn bundle_content_root(bundle: &BundleManifest) -> String {
    let mut hasher = blake3::Hasher::new_derive_key("AriaEngine V3 portable bundle root");
    for value in [
        bundle.game_id.as_str(),
        bundle.game_version.as_str(),
        bundle.game_title.as_str(),
        bundle.save_namespace.as_str(),
        bundle.ariac_blake3.as_str(),
        bundle.pak_blake3.as_str(),
        bundle.pak_content_root_blake3.as_str(),
        bundle.pack_id.as_str(),
        match bundle.pak_profile {
            BuildProfile::Dev => "dev",
            BuildProfile::Signed => "signed",
            BuildProfile::Protected => "protected",
        },
    ] {
        hasher.update(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.update(
        &u32::try_from(bundle.font_assets.len())
            .expect("font asset count fits u32")
            .to_le_bytes(),
    );
    for font in &bundle.font_assets {
        hasher.update(
            &u32::try_from(font.len())
                .expect("font asset path length fits u32")
                .to_le_bytes(),
        );
        hasher.update(font.as_bytes());
    }
    hasher.update(&bundle.language_major.to_le_bytes());
    hasher.update(&bundle.language_minor.to_le_bytes());
    hasher.update(&bundle.vm_abi_version.to_le_bytes());
    hasher.update(&bundle.logical_width.to_le_bytes());
    hasher.update(&bundle.logical_height.to_le_bytes());
    hasher.update(&bundle.ariac_size.to_le_bytes());
    hasher.update(&bundle.pak_size.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

fn build_pak(
    profile: BuildProfile,
    game_id: &str,
    pack_id: &str,
    assets: Vec<AssetInput>,
    keys: PakBuildKeys,
) -> Result<(Vec<u8>, String)> {
    if profile == BuildProfile::Dev {
        let package =
            PakPackage::build(PakBuildInput::new(pack_id, game_id, PakRole::Boot, assets))?;
        let archive = PakPackage::open(&package, None)?;
        return Ok((package, archive.content_root().to_owned()));
    }

    let license_policy = if profile == BuildProfile::Protected {
        // The policy is part of the signed pack manifest. A project-specific
        // entitlement service can issue a shorter lease, but the offline
        // contract remains explicit and never forces a network at launch.
        LicensePolicy::offline(7 * 24 * 60 * 60, 24 * 60 * 60)
    } else {
        LicensePolicy::none()
    };
    let package = PakPackage::build(PakBuildInput {
        pack_id: pack_id.to_owned(),
        game_id: game_id.to_owned(),
        role: PakRole::Boot,
        subtype: "base".to_owned(),
        dependencies: Vec::new(),
        priority: 0,
        assets,
        profile: profile.as_pak_profile(),
        signing_key: keys.signing.clone(),
        encryption_key: keys.encryption.clone(),
        license_policy,
    })?;
    let mut provider = StaticPakKeyProvider::new();
    if let Some(signing) = keys.signing.as_ref() {
        provider = provider.with_signing_key(signing);
    }
    if let Some(encryption) = keys.encryption.as_ref() {
        provider = provider.with_encryption_key(encryption);
    }
    let archive = PakPackage::open(&package, Some(&provider))?;
    Ok((package, archive.content_root().to_owned()))
}

pub(crate) fn resolve_pak_keys(
    profile: BuildProfile,
    signing_value: Option<&str>,
    encryption_value: Option<&str>,
) -> Result<PakBuildKeys> {
    let signing_value = signing_value
        .map(str::to_owned)
        .or_else(|| std::env::var("ARIA_PAK_SIGNING_KEY").ok());
    let encryption_value = encryption_value
        .map(str::to_owned)
        .or_else(|| std::env::var("ARIA_PAK_ENCRYPTION_KEY").ok());
    let signing = if profile.as_pak_profile().requires_signature() {
        let value = signing_value
            .as_deref()
            .context("signed/protected profile requires --signing-key or ARIA_PAK_SIGNING_KEY")?;
        let (key_id, hex) = split_key_value(value, "publisher");
        Some(PakSigningKey::from_hex(key_id, &hex)?)
    } else {
        None
    };
    let encryption = if profile.as_pak_profile().requires_encryption() {
        let value = encryption_value
            .as_deref()
            .context("protected profile requires --encryption-key or ARIA_PAK_ENCRYPTION_KEY")?;
        let (key_id, hex) = split_key_value(value, "content");
        Some(PakEncryptionKey::from_hex(key_id, &hex)?)
    } else {
        None
    };
    Ok(PakBuildKeys {
        signing,
        encryption,
    })
}

fn split_key_value(value: &str, default_id: &str) -> (String, String) {
    value.split_once(':').map_or_else(
        || (default_id.to_owned(), value.to_owned()),
        |(key_id, key)| (key_id.to_owned(), key.to_owned()),
    )
}

fn collect_assets(inventory: &AssetInventory) -> Result<Vec<AssetInput>> {
    let mut assets = Vec::new();
    for (logical_path, disk_path) in inventory.iter() {
        assets.push(AssetInput {
            logical_path: logical_path.to_owned(),
            bytes: fs::read(disk_path)?,
        });
    }
    Ok(assets)
}

fn write_pwa_shell(destination: &Path, runtime_package: Option<&Path>) -> Result<bool> {
    const FILES: &[(&str, &str)] = &[
        ("index.html", include_str!("../../aria-web/pwa/index.html")),
        ("app.css", include_str!("../../aria-web/pwa/app.css")),
        (
            "manifest.webmanifest",
            include_str!("../../aria-web/pwa/manifest.webmanifest"),
        ),
        ("main.js", include_str!("../../aria-web/pwa/main.js")),
        (
            "web-audio.js",
            include_str!("../../aria-web/pwa/web-audio.js"),
        ),
        (
            "web-renderer.js",
            include_str!("../../aria-web/pwa/web-renderer.js"),
        ),
        (
            "save-store.js",
            include_str!("../../aria-web/pwa/save-store.js"),
        ),
    ];
    for (name, contents) in FILES {
        fs::write(destination.join(name), contents)?;
    }
    let runtime_files = if let Some(runtime_package) = runtime_package {
        copy_web_runtime(runtime_package, &destination.join("pkg"))?
    } else {
        Vec::new()
    };
    let cached_runtime = serde_json::to_string(
        &runtime_files
            .iter()
            .map(|name| format!("./pkg/{name}"))
            .collect::<Vec<_>>(),
    )?;
    let service_worker = include_str!("../../aria-web/pwa/service-worker.js")
        .replace("__ARIA_WEB_RUNTIME_CACHE__", &cached_runtime);
    fs::write(destination.join("service-worker.js"), service_worker)?;
    Ok(!runtime_files.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::pak::PakArchive;

    fn project(root: &Path) {
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("aria.toml"),
            "schema = 3\n\
             [game]\n\
             id = \"jp.example.test\"\n\
             version = \"3.0.0\"\n\
             title = \"test\"\n\
             [runtime]\n\
             entry = \"scripts/main.aria\"\n\
             logical_width = 1280\n\
             logical_height = 720\n\
             asset_roots = [\"assets\"]\n\
             fonts = []\n\
             save_namespace = \"test\"\n",
        )
        .unwrap();
        fs::write(
            root.join("scripts/main.aria"),
            "# aria-version: 3.0\nミオ「テスト。」\nend\n",
        )
        .unwrap();
        fs::write(root.join("assets/data.txt"), "asset").unwrap();
    }

    #[test]
    fn web_build_contains_checked_bytecode_pak_and_thin_shell() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let out = temp.path().join("output");
        build_project(temp.path(), BuildTarget::Web, Some(&out)).unwrap();
        aria_core::CompiledProgram::decode(&fs::read(out.join("game.ariac")).unwrap()).unwrap();
        PakArchive::open(&fs::read(out.join("game.ariapak")).unwrap()).unwrap();
        assert!(out.join("service-worker.js").is_file());
        assert!(out.join("save-store.js").is_file());
        assert!(out.join("web-renderer.js").is_file());
        let bundle: BundleManifest =
            serde_json::from_slice(&fs::read(out.join("bundle.aria.json")).unwrap()).unwrap();
        assert_eq!(bundle.schema_version, 5);
        assert_eq!(bundle.content_root_blake3, bundle_content_root(&bundle));
        let manifest: BuildManifest =
            serde_json::from_slice(&fs::read(out.join("build-manifest.json")).unwrap()).unwrap();
        assert!(manifest.web_runtime_package_required);
        assert!(
            fs::read_to_string(out.join("service-worker.js"))
                .unwrap()
                .contains("const RUNTIME = [];")
        );
        assert!(
            fs::read_to_string(out.join("service-worker.js"))
                .unwrap()
                .contains("./bundle.aria.json")
        );
    }

    #[test]
    fn pwa_shell_includes_a_prebuilt_wasm_runtime_and_precaches_it() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("runtime");
        let out = temp.path().join("output");
        fs::create_dir(&runtime).unwrap();
        fs::write(
            runtime.join("aria_web.js"),
            "export default async function() {}\nexport class WebRuntime {}\nexport class WebPak {}\n// aria_web_bg.wasm",
        )
        .unwrap();
        fs::write(runtime.join("aria_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
        fs::create_dir(&out).unwrap();
        assert!(write_pwa_shell(&out, Some(&runtime)).unwrap());
        assert!(out.join("pkg/aria_web.js").is_file());
        assert!(out.join("web-renderer.js").is_file());
        let worker = fs::read_to_string(out.join("service-worker.js")).unwrap();
        assert!(worker.contains("./pkg/aria_web.js"));
        assert!(worker.contains("./pkg/aria_web_bg.wasm"));
        assert!(worker.contains("./web-renderer.js"));
        assert!(worker.contains("./bundle.aria.json"));
    }

    #[test]
    fn release_build_rejects_host_compatibility_commands() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        fs::write(
            temp.path().join("scripts/main.aria"),
            "# aria-version: 3.0\nstrict on\nui title, \"未実装\"\nend\n",
        )
        .unwrap();

        let dev_out = temp.path().join("dev-output");
        build_project(temp.path(), BuildTarget::Web, Some(&dev_out)).unwrap();
        let release_out = temp.path().join("release-output");
        let error =
            build_project_with_release(temp.path(), BuildTarget::Web, Some(&release_out), true)
                .unwrap_err();
        assert!(error.to_string().contains("unsupported runtime command"));
        assert!(!release_out.exists());
    }

    #[test]
    fn portable_bundle_bytes_are_identical_across_windows_and_linux_wrappers() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let windows = temp.path().join("windows-output");
        let linux = temp.path().join("linux-output");
        build_project(temp.path(), BuildTarget::WindowsX64, Some(&windows)).unwrap();
        build_project(temp.path(), BuildTarget::LinuxX64, Some(&linux)).unwrap();
        for name in ["game.ariac", "game.ariapak", "bundle.aria.json"] {
            assert_eq!(
                fs::read(windows.join(name)).unwrap(),
                fs::read(linux.join(name)).unwrap()
            );
        }
        let pak = PakArchive::open(&fs::read(linux.join("game.ariapak")).unwrap()).unwrap();
        assert_eq!(pak.read("assets/data.txt").unwrap(), b"asset");
    }

    #[test]
    fn signed_profile_writes_manifest_backed_ariapak() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let signer = PakSigningKey::from_bytes("publisher", [7; 32]).unwrap();
        let out = temp.path().join("signed-output");
        build_project_with_profile_and_keys(
            temp.path(),
            BuildTarget::LinuxX64,
            Some(&out),
            false,
            BuildProfile::Signed,
            PakBuildKeys {
                signing: Some(signer.clone()),
                encryption: None,
            },
        )
        .unwrap();
        let bundle: BundleManifest =
            serde_json::from_slice(&fs::read(out.join("bundle.aria.json")).unwrap()).unwrap();
        assert_eq!(bundle.pak_profile, BuildProfile::Signed);
        assert_eq!(bundle.pack_id, "jp.example.test.boot");
        let provider = StaticPakKeyProvider::new().with_signing_key(&signer);
        let package = PakPackage::open(
            &fs::read(out.join("game.ariapak")).unwrap(),
            Some(&provider),
        )
        .unwrap();
        assert_eq!(package.manifest().pack_id, bundle.pack_id);
        assert_eq!(package.content_root(), bundle.pak_content_root_blake3);
    }
}
