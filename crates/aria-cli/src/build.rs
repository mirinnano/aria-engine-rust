use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use aria_core::Severity;
use aria_core::pak::AssetInput;
use aria_protection::{
    LicensePolicy, PakBuildInput, PakEncryptionKey, PakPackage, PakProfile, PakRole, PakSigningKey,
    StaticPakKeyProvider,
};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::package_runtime::{
    copy_native_player, copy_web_runtime, native_player_filename, resolve_native_player,
    resolve_web_runtime, validate_native_player, validate_web_runtime_package,
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
    /// Namespaces intentionally retired by this release. Players erase only
    /// these exact names before they open `save_namespace`.
    #[serde(default)]
    pub legacy_save_namespaces: Vec<String>,
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
    /// The ordered pack set. The legacy top-level pak fields describe the
    /// primary pack (the first entry) so older tooling can still diagnose a
    /// package before it learns the split-pack contract.
    #[serde(default)]
    pub pak_packs: Vec<BundlePakManifest>,
    pub content_root_blake3: String,
    pub integrity: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundlePakManifest {
    pub pack_id: String,
    pub role: PakRole,
    pub file: String,
    pub blake3: String,
    pub size: u64,
    pub content_root_blake3: String,
    /// Logical paths assigned to this pack. This lets Web mount the smallest
    /// possible pack before asking the archive reader to probe every pack.
    pub assets: Vec<String>,
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
    command_with_profile(
        path,
        target,
        out,
        release,
        BuildProfile::Dev,
        None,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn command_with_profile(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    release: bool,
    profile: BuildProfile,
    signing_key: Option<&str>,
    encryption_key: Option<&str>,
    build_player: Option<bool>,
    player: Option<&Path>,
) -> Result<u8> {
    command_with_profile_and_runtime_overrides(
        path,
        target,
        out,
        release,
        profile,
        signing_key,
        encryption_key,
        build_player,
        player,
        None,
        None,
    )
}

/// Builds a content-limited edition without rewriting the source manifest.
/// The overrides are logical project values, not host paths, and are
/// validated before compilation.
#[allow(clippy::too_many_arguments)]
pub fn command_with_profile_and_runtime_overrides(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    release: bool,
    profile: BuildProfile,
    signing_key: Option<&str>,
    encryption_key: Option<&str>,
    build_player: Option<bool>,
    player: Option<&Path>,
    entry: Option<&str>,
    save_namespace: Option<&str>,
) -> Result<u8> {
    let keys = resolve_pak_keys(profile, signing_key, encryption_key)?;
    let output = build_project_with_profile_and_keys_and_runtime_overrides(
        path,
        target,
        out,
        release,
        profile,
        keys,
        build_player,
        player,
        entry,
        save_namespace,
    )?;
    println!("built {}", output.display());

    // C5: Print size ratchet table.
    print_size_table(&output, target)?;

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
    build_project_with_profile_and_keys(path, target, out, false, profile, keys, None, None)
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
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_project_with_profile_and_keys(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    release: bool,
    profile: BuildProfile,
    pak_keys: PakBuildKeys,
    build_player: Option<bool>,
    player: Option<&Path>,
) -> Result<PathBuf> {
    build_project_with_profile_and_keys_and_runtime_overrides(
        path,
        target,
        out,
        release,
        profile,
        pak_keys,
        build_player,
        player,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_project_with_profile_and_keys_and_runtime_overrides(
    path: &Path,
    target: BuildTarget,
    out: Option<&Path>,
    release: bool,
    profile: BuildProfile,
    pak_keys: PakBuildKeys,
    build_player: Option<bool>,
    player: Option<&Path>,
    entry: Option<&str>,
    save_namespace: Option<&str>,
) -> Result<PathBuf> {
    let project = LoadedProject::load(path)?.with_runtime_overrides(entry, save_namespace)?;
    let compiled = project.compile()?;
    for diagnostic in &compiled.diagnostics {
        eprintln!("{diagnostic}");
    }
    if compiled.has_errors() {
        bail!("project has compiler errors");
    }
    if release && !crate::release::has_release_language(&compiled) {
        bail!("release build requires source in the single 'aria;' language");
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
        eprintln!("warning: build contains {warning_count} compiler warning(s)");
    }
    let program = compiled.program.context("compiler produced no program")?;
    let ariac = program.encode()?;
    let asset_inventory = project.asset_inventory()?;
    project.validate_bundled_fonts(&asset_inventory, release)?;
    let assets_by_role = collect_assets_by_role(&project, &asset_inventory)?;

    let ariac_blake3 = blake3::hash(&ariac).to_hex().to_string();
    let mut built_packs = Vec::new();
    for role in [PakRole::Boot, PakRole::Hot, PakRole::Cold, PakRole::Overlay] {
        let Some(assets) = assets_by_role.get(&role) else {
            continue;
        };
        if assets.is_empty() {
            continue;
        }
        let pack_id = format!("{}.{}", project.manifest.game.id, role.as_str());
        let (bytes, content_root) = build_pak(
            profile,
            &project.manifest.game.id,
            &pack_id,
            role,
            assets.clone(),
            pak_keys.clone(),
        )?;
        let file = if built_packs.is_empty() {
            "game.ariapak".to_owned()
        } else {
            format!("game.{}.ariapak", role.as_str())
        };
        built_packs.push((
            BundlePakManifest {
                pack_id,
                role,
                file,
                blake3: blake3::hash(&bytes).to_hex().to_string(),
                size: u64::try_from(bytes.len()).context("asset pak is too large")?,
                content_root_blake3: content_root,
                assets: assets
                    .iter()
                    .map(|asset| asset.logical_path.clone())
                    .collect(),
            },
            bytes,
        ));
    }
    if built_packs.is_empty() {
        bail!("asset inventory produced no packable assets");
    }
    let primary = &built_packs[0].0;
    let pack_id = primary.pack_id.clone();
    let pak_blake3 = primary.blake3.clone();
    let pak_content_root = primary.content_root_blake3.clone();
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
        legacy_save_namespaces: project.manifest.runtime.legacy_save_namespaces.clone(),
        font_assets: project.manifest.runtime.fonts.clone(),
        ariac_blake3,
        ariac_size: u64::try_from(ariac.len()).context("compiled program is too large")?,
        pak_blake3,
        pak_size: primary.size,
        pak_content_root_blake3: pak_content_root,
        pak_profile: profile,
        pack_id,
        pak_packs: built_packs
            .iter()
            .map(|(manifest, _)| manifest.clone())
            .collect(),
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

    // C1/C2: Resolve target executables and browser glue before creating any
    // staging directory. A failed release preflight must not leave a half-written
    // package that a later invocation could mistake for a valid artifact.
    let auto_build_enabled = build_player.unwrap_or(release);
    let native_player = if target == BuildTarget::Web {
        None
    } else if let Some(path) = player {
        // --player flag takes highest priority.
        Some(validate_native_player(path, target)?)
    } else if let Some(path) = std::env::var_os("ARIA_PLAYER_BINARY").map(PathBuf::from) {
        // ARIA_PLAYER_BINARY env next.
        Some(validate_native_player(&path, target)?)
    } else if auto_build_enabled {
        // Auto-build the native Player.
        let built = build_native_player(target, release)?;
        Some(validate_native_player(&built, target)?)
    } else {
        // Fall through to existing logic (dev build, current_exe, etc.).
        resolve_native_player(target, release)?
    };

    // C2: Web runtime auto-build.
    let web_runtime = if target == BuildTarget::Web {
        resolve_web_runtime_with_auto_build(release)?
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
    for (pack, bytes) in &built_packs {
        fs::write(staging.join(&pack.file), bytes)?;
    }
    fs::write(staging.join("bundle.aria.json"), &bundle_bytes)?;

    let player_binary_included = if let Some(player) = native_player {
        copy_native_player(&player, &staging.join(native_player_filename(target)))?;
        true
    } else {
        false
    };
    let web_runtime_included = if target == BuildTarget::Web {
        write_web_presentation(
            &staging,
            web_runtime.as_deref(),
            &project.root.join(&project.manifest.presentation.frontend),
        )?
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
        &u32::try_from(bundle.legacy_save_namespaces.len())
            .expect("legacy save namespace count fits u32")
            .to_le_bytes(),
    );
    for namespace in &bundle.legacy_save_namespaces {
        hasher.update(
            &u32::try_from(namespace.len())
                .expect("legacy save namespace length fits u32")
                .to_le_bytes(),
        );
        hasher.update(namespace.as_bytes());
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
    hasher.update(
        &u32::try_from(bundle.pak_packs.len())
            .expect("pack count fits u32")
            .to_le_bytes(),
    );
    for pack in &bundle.pak_packs {
        for value in [
            pack.pack_id.as_str(),
            pack.role.as_str(),
            pack.file.as_str(),
            pack.blake3.as_str(),
            pack.content_root_blake3.as_str(),
        ] {
            hasher.update(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_le_bytes());
            hasher.update(value.as_bytes());
        }
        hasher.update(&pack.size.to_le_bytes());
        hasher.update(
            &u32::try_from(pack.assets.len())
                .expect("pack asset count fits u32")
                .to_le_bytes(),
        );
        for asset in &pack.assets {
            hasher.update(
                &u32::try_from(asset.len())
                    .expect("pack asset path length fits u32")
                    .to_le_bytes(),
            );
            hasher.update(asset.as_bytes());
        }
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
    role: PakRole,
    assets: Vec<AssetInput>,
    keys: PakBuildKeys,
) -> Result<(Vec<u8>, String)> {
    if profile == BuildProfile::Dev {
        let package = PakPackage::build(PakBuildInput::new(pack_id, game_id, role, assets))?;
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
        role,
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

fn collect_assets_by_role(
    project: &LoadedProject,
    inventory: &AssetInventory,
) -> Result<BTreeMap<PakRole, Vec<AssetInput>>> {
    let mut assets = BTreeMap::<PakRole, Vec<AssetInput>>::new();
    for (logical_path, disk_path) in inventory.iter() {
        let role = project
            .manifest
            .runtime
            .asset_pack_roles
            .get(logical_path)
            .map(|role| match role.as_str() {
                "boot" => Ok(PakRole::Boot),
                "hot" => Ok(PakRole::Hot),
                "cold" => Ok(PakRole::Cold),
                "overlay" => Ok(PakRole::Overlay),
                unsupported => Err(anyhow::anyhow!(
                    "unsupported asset pack role '{unsupported}' for '{logical_path}'"
                )),
            })
            .transpose()?
            .unwrap_or(PakRole::Boot);
        assets.entry(role).or_default().push(AssetInput {
            logical_path: logical_path.to_owned(),
            bytes: fs::read(disk_path)?,
        });
    }
    Ok(assets)
}

/// Builds the game-owned React presentation into the web package. The engine
/// supplies only the scene renderer, audio/save adapters, and WASM runtime;
/// it never falls back to a generic visual shell.
fn write_web_presentation(
    destination: &Path,
    runtime_package: Option<&Path>,
    frontend: &Path,
) -> Result<bool> {
    let frontend_metadata = fs::symlink_metadata(frontend).with_context(|| {
        format!(
            "presentation.frontend directory is missing: {}",
            frontend.display()
        )
    })?;
    if frontend_metadata.file_type().is_symlink() || !frontend_metadata.is_dir() {
        bail!(
            "presentation.frontend must be a real directory: {}",
            frontend.display()
        );
    }
    let package = frontend.join("package.json");
    if !package.is_file() {
        bail!(
            "presentation.frontend must contain package.json: {}",
            package.display()
        );
    }

    if let Some(prebuilt) = std::env::var_os("ARIA_PRESENTATION_PREBUILT_DIR") {
        // The desktop release wrapper builds Vite once into a fingerprinted
        // cache and passes it here. This keeps `aria build` useful as a
        // standalone command while avoiding a second npm invocation in the
        // Tauri beforeBuild hook.
        let prebuilt = PathBuf::from(prebuilt);
        let metadata = fs::symlink_metadata(&prebuilt).with_context(|| {
            format!(
                "ARIA_PRESENTATION_PREBUILT_DIR does not exist: {}",
                prebuilt.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!(
                "ARIA_PRESENTATION_PREBUILT_DIR must be a real directory: {}",
                prebuilt.display()
            );
        }
        if !prebuilt.join("index.html").is_file() {
            bail!(
                "ARIA_PRESENTATION_PREBUILT_DIR is missing index.html: {}",
                prebuilt.display()
            );
        }
        copy_directory_contents(&prebuilt, destination)?;
    } else {
        let presentation_output = destination.join(".aria-presentation");
        fs::create_dir_all(&presentation_output)?;
        let absolute_output = presentation_output.canonicalize().with_context(|| {
            format!(
                "cannot resolve temporary presentation output {}",
                presentation_output.display()
            )
        })?;
        let mut npm = Command::new("npm");
        npm.arg("run")
            .arg("build")
            .current_dir(frontend)
            .env("ARIA_PRESENTATION_OUT_DIR", &absolute_output);
        if let Some(value) = std::env::var_os("ARIA_PAK_VERIFICATION_KEY_ID") {
            npm.env("VITE_ARIA_PAK_VERIFICATION_KEY_ID", value);
        }
        if let Some(value) = std::env::var_os("ARIA_PAK_VERIFICATION_KEY_HEX") {
            npm.env("VITE_ARIA_PAK_VERIFICATION_KEY_HEX", value);
        }
        let status = npm.status().with_context(|| {
            format!(
                "cannot start npm to build presentation.frontend {}",
                frontend.display()
            )
        })?;
        if !status.success() {
            bail!(
                "presentation frontend build failed ({}); run 'npm install' then 'npm run build' in {}",
                status,
                frontend.display()
            );
        }
        if !absolute_output.join("index.html").is_file() {
            bail!(
                "presentation frontend did not produce index.html in {}",
                absolute_output.display()
            );
        }
        copy_directory_contents(&absolute_output, destination)?;
        fs::remove_dir_all(&presentation_output)?;
    }

    const ADAPTERS: &[(&str, &str)] = &[
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
    for (name, contents) in ADAPTERS {
        fs::write(destination.join(name), contents)?;
    }
    let runtime_files = if let Some(runtime_package) = runtime_package {
        copy_web_runtime(runtime_package, &destination.join("pkg"))?
    } else {
        Vec::new()
    };
    Ok(!runtime_files.is_empty())
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        if entry.path() == source {
            continue;
        }
        if entry.file_type().is_symlink() {
            bail!(
                "presentation build output must not contain symbolic links: {}",
                entry.path().display()
            );
        }
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "cannot copy presentation asset {} to {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

// ── C1: Player auto-build ───────────────────────────────────────────

/// Target triples for cross-compilation.
fn target_triple(target: BuildTarget) -> &'static [&'static str] {
    match target {
        BuildTarget::WindowsX64 => &["x86_64-pc-windows-msvc"],
        BuildTarget::LinuxX64 | BuildTarget::SteamdeckX64 => &["x86_64-unknown-linux-gnu"],
        BuildTarget::MacosUniversal => &["x86_64-apple-darwin", "aarch64-apple-darwin"],
        BuildTarget::Web => &[],
    }
}

/// Locates the engine source root via `ARIA_ENGINE_SRC` or repo detection.
fn find_engine_root() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("ARIA_ENGINE_SRC") {
        let p = PathBuf::from(path);
        if p.join("Cargo.toml").is_file() {
            return Ok(p);
        }
        bail!(
            "ARIA_ENGINE_SRC points to a directory without Cargo.toml: {}",
            p.display()
        );
    }
    // Fallback: detect whether we're running from a repo checkout.
    let exe = std::env::current_exe().ok();
    let mut current = match exe {
        Some(p) => p,
        None => bail!("cannot locate current executable for engine root detection"),
    };
    loop {
        let cargo = current.join("Cargo.toml");
        if cargo.is_file()
            && let Ok(contents) = fs::read_to_string(&cargo)
            && (contents.contains("aria-core") || contents.contains("aria-engine"))
        {
            return Ok(current);
        }
        if !current.pop() {
            break;
        }
    }
    bail!("cannot find engine source: set ARIA_ENGINE_SRC or run from the repo checkout");
}

/// Auto-builds a native Player binary for the given target.
fn build_native_player(target: BuildTarget, release: bool) -> Result<PathBuf> {
    let engine_root = find_engine_root()?;
    let triples = target_triple(target);

    if triples.is_empty() {
        bail!("auto-build not supported for web target");
    }

    let cargo = if cfg!(windows) { "cargo.exe" } else { "cargo" };

    // Check that the required cargo target(s) are installed.
    for triple in triples {
        let mut cmd = Command::new(cargo);
        cmd.args([
            "build",
            "-p",
            "aria-cli",
            "--features",
            "desktop-player",
            "--target",
            triple,
        ]);
        if release {
            cmd.arg("--release");
        }
        cmd.arg("--dry-run")
            .current_dir(&engine_root)
            .status()
            .with_context(|| format!("cannot run cargo to check target {triple}"))?;
        // --dry-run may fail for other reasons; we just want to check if the target is recognized.
        // A more reliable check: try `rustc --print target-spec-json --target <triple>`.
    }
    // Build each slice.
    let mut built_binaries = Vec::new();
    for triple in triples {
        println!("  Building Player for {triple}...");
        let mut cmd = Command::new(cargo);
        cmd.args(["build", "-p", "aria-cli", "--features", "desktop-player"]);
        if release {
            cmd.arg("--release");
        }
        cmd.arg("--target").arg(triple).current_dir(&engine_root);
        let status = cmd.status().with_context(|| {
            format!(
                "failed to start cargo build for {triple}; \
                     install the target with: rustup target add {triple}"
            )
        })?;
        if !status.success() {
            bail!(
                "cargo build for {triple} failed (exit {status}); \
                 install the target with: rustup target add {triple}"
            );
        }

        let target_dir = engine_root.join("target").join(triple);
        let profile_dir = if release { "release" } else { "debug" };
        let bin_name = if cfg!(windows) { "aria.exe" } else { "aria" };
        let bin = target_dir.join(profile_dir).join(bin_name);
        if !bin.is_file() {
            bail!(
                "cargo build did not produce expected binary: {}",
                bin.display()
            );
        }
        built_binaries.push(bin);
    }

    // For macos-universal, FAT the binaries.
    if target == BuildTarget::MacosUniversal {
        fat_macos_binaries(&built_binaries, &engine_root)
    } else {
        // Single binary — copy to a known location.
        let single = built_binaries.into_iter().next().unwrap();
        Ok(single)
    }
}

/// Creates a FAT Mach-O binary from multiple slices using lipo or llvm-lipo.
fn fat_macos_binaries(binaries: &[PathBuf], engine_root: &Path) -> Result<PathBuf> {
    if binaries.len() < 2 {
        bail!(
            "macos-universal requires at least 2 slices, got {}",
            binaries.len()
        );
    }

    // Find lipo or llvm-lipo.
    let lipo_path = find_binary("lipo")
        .or_else(|| find_binary("llvm-lipo"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "neither `lipo` nor `llvm-lipo` found on PATH; \
                 install Xcode command-line tools (macOS) or llvm (Linux) for FAT Mach-O creation"
            )
        })?;

    let out_dir = engine_root.join("target/macos-universal");
    fs::create_dir_all(&out_dir)?;
    let output = out_dir.join("aria-player");

    let mut cmd = Command::new(&lipo_path);
    cmd.arg("-create");
    for bin in binaries {
        cmd.arg(bin);
    }
    cmd.arg("-output").arg(&output);

    let status = cmd
        .status()
        .with_context(|| format!("failed to run {} to create FAT binary", lipo_path.display()))?;
    if !status.success() {
        bail!(
            "{} failed to create FAT binary (exit {status})",
            lipo_path.display()
        );
    }
    if !output.is_file() {
        bail!("FAT binary not created at {}", output.display());
    }
    println!("  FAT binary created: {}", output.display());
    Ok(output)
}

/// Finds an executable on PATH.
fn find_binary(name: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

// ── C2: Web runtime auto-build ──────────────────────────────────────

const WEB_RUNTIME_DIR: &str = "target/aria-web-runtime/release";

/// Resolves the web runtime, auto-building if necessary.
fn resolve_web_runtime_with_auto_build(release: bool) -> Result<Option<PathBuf>> {
    // 1. ARIA_WEB_RUNTIME_DIR env (existing behavior via resolve_web_runtime).
    if std::env::var_os("ARIA_WEB_RUNTIME_DIR").is_some() {
        return resolve_web_runtime(release);
    }

    // 2. Check if cached runtime exists and is fresh. The cache lives under
    // the engine root, never the caller's CWD: `cargo test` runs with the
    // crate dir as CWD and a relative path would strand the cache there.
    let runtime_path = find_engine_root()?.join(WEB_RUNTIME_DIR);

    // 2. Check if cached runtime exists and is fresh.
    if runtime_path.is_dir() {
        // Check staleness: is the cached runtime older than the aria-web source?
        if is_web_runtime_fresh(&runtime_path)? {
            return Ok(Some(runtime_path));
        }
    }

    // 3. Auto-build.
    if release {
        println!("  Auto-building web runtime (release mode)...");
    } else {
        println!("  Auto-building web runtime...");
    }
    auto_build_web_runtime(&runtime_path)?;
    Ok(Some(runtime_path))
}

/// Checks whether the cached web runtime is fresh relative to aria-web source.
fn is_web_runtime_fresh(runtime_path: &Path) -> Result<bool> {
    // Get mtime of the most recent file in the runtime cache.
    let mut runtime_mtime = None;
    for entry in walkdir::WalkDir::new(runtime_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let mt = entry
                .metadata()
                .with_context(|| format!("cannot read metadata of {}", entry.path().display()))?
                .modified()
                .with_context(|| format!("cannot get mtime of {}", entry.path().display()))?;
            runtime_mtime = Some(runtime_mtime.map_or(mt, |m: std::time::SystemTime| m.max(mt)));
        }
    }
    let Some(runtime_mt) = runtime_mtime else {
        return Ok(false);
    };

    // Get mtime of the most recent aria-web source file.
    let mut source_mtime: Option<std::time::SystemTime> = None;
    let engine_root = find_engine_root()?;
    let web_src = engine_root.join("crates/aria-web/src");
    if web_src.is_dir() {
        for entry in walkdir::WalkDir::new(&web_src)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "rs")
            {
                let mt = entry
                    .metadata()
                    .with_context(|| format!("cannot read metadata of {}", entry.path().display()))?
                    .modified()
                    .with_context(|| format!("cannot get mtime of {}", entry.path().display()))?;
                source_mtime = Some(source_mtime.map_or(mt, |m: std::time::SystemTime| m.max(mt)));
            }
        }
    }
    let Some(source_mt) = source_mtime else {
        return Ok(true); // no source to compare against → assume fresh
    };

    // Runtime is fresh if it's newer than the source.
    Ok(runtime_mt >= source_mt)
}

/// Auto-builds the web runtime (wasm + wasm-bindgen).
fn auto_build_web_runtime(output: &Path) -> Result<()> {
    let engine_root = find_engine_root()?;
    let cargo = if cfg!(windows) { "cargo.exe" } else { "cargo" };

    println!("  Building aria-web wasm module...");
    let status = Command::new(cargo)
        .args([
            "build",
            "-p",
            "aria-web",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ])
        .current_dir(&engine_root)
        .status()
        .context(
            "failed to start cargo build for aria-web; install wasm32-unknown-unknown target",
        )?;
    if !status.success() {
        bail!(
            "cargo build for aria-web failed; \
             install the target with: rustup target add wasm32-unknown-unknown"
        );
    }

    // Find wasm-bindgen CLI.
    let wb_path = find_binary("wasm-bindgen").ok_or_else(|| {
        anyhow::anyhow!(
            "wasm-bindgen CLI not found on PATH; install with: \
                 cargo install wasm-bindgen-cli --version 0.2.126 --locked"
        )
    })?;

    println!("  Running wasm-bindgen...");
    fs::create_dir_all(output)?;
    let wasm_input = engine_root.join("target/wasm32-unknown-unknown/release/aria_web.wasm");
    let status = Command::new(&wb_path)
        .args(["--target", "web", "--out-dir"])
        .arg(output)
        .arg("--out-name")
        .arg("aria_web")
        .arg(&wasm_input)
        .status()
        .with_context(|| {
            "failed to run wasm-bindgen; \
             install with: cargo install wasm-bindgen-cli --version 0.2.126 --locked"
                .to_owned()
        })?;
    if !status.success() {
        bail!(
            "wasm-bindgen failed (exit {status}); \
             install with: cargo install wasm-bindgen-cli --version 0.2.126 --locked"
        );
    }

    // Validate the output.
    validate_web_runtime_package(output)?;
    println!("  Web runtime built: {}", output.display());
    Ok(())
}

// ── C5: Size ratchet table ──────────────────────────────────────────

/// Prints a size table for the build artifacts.
fn print_size_table(output: &Path, target: BuildTarget) -> Result<()> {
    println!();
    println!("┌─────────────────────────┬────────────┐");
    println!("│ Artifact                │ Size       │");
    println!("├─────────────────────────┼────────────┤");

    for name in ["game.ariac", "game.ariapak", "bundle.aria.json"] {
        let path = output.join(name);
        if path.is_file() {
            let size = fs::metadata(&path)?.len();
            println!("│ {:<23} │ {:>10} │", name, format_bytes(size));
        }
    }
    if let Some(bundle) = fs::read(output.join("bundle.aria.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BundleManifest>(&bytes).ok())
    {
        for pack in bundle.pak_packs.iter().skip(1) {
            let path = output.join(&pack.file);
            if path.is_file() {
                println!(
                    "│ {:<23} │ {:>10} │",
                    pack.file,
                    format_bytes(fs::metadata(path)?.len())
                );
            }
        }
    }

    if target == BuildTarget::Web {
        // Report wasm size.
        let wasm = output.join("pkg/aria_web_bg.wasm");
        if wasm.is_file() {
            let size = fs::metadata(&wasm)?.len();
            println!(
                "│ {:<23} │ {:>10} │",
                "aria_web_bg.wasm",
                format_bytes(size)
            );
        }
    } else {
        // Report player size.
        let player_name = native_player_filename(target);
        let player = output.join(player_name);
        if player.is_file() {
            let size = fs::metadata(&player)?.len();
            println!("│ {:<23} │ {:>10} │", player_name, format_bytes(size));
        }
    }

    println!("└─────────────────────────┴────────────┘");
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::pak::PakArchive;

    fn project(root: &Path) {
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        write_test_presentation(root);
        fs::write(
            root.join("aria.toml"),
            "schema = 4\n\
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
             save_namespace = \"test\"\n\
             [presentation]\n\
             frontend = \"ui\"\n",
        )
        .unwrap();
        fs::write(
            root.join("scripts/main.aria"),
            "aria;\nentry start;\nscene start { say ミオ: \"テスト。\"; await advance; end; }\n",
        )
        .unwrap();
        fs::write(root.join("assets/data.txt"), "asset").unwrap();
    }

    fn write_test_presentation(root: &Path) {
        let frontend = root.join("ui");
        fs::create_dir_all(&frontend).unwrap();
        fs::write(
            frontend.join("package.json"),
            r#"{"private":true,"scripts":{"build":"node build.mjs"}}"#,
        )
        .unwrap();
        fs::write(
            frontend.join("build.mjs"),
            r#"import { mkdir, writeFile } from "node:fs/promises";
const output = process.env.ARIA_PRESENTATION_OUT_DIR;
if (!output) throw new Error("ARIA_PRESENTATION_OUT_DIR is required");
await mkdir(output, { recursive: true });
await writeFile(`${output}/index.html`, "<!doctype html><title>test presentation</title>");
await writeFile(`${output}/service-worker.js`, "self.addEventListener('fetch', () => {});");
"#,
        )
        .unwrap();
    }

    #[test]
    fn web_build_contains_checked_bytecode_pak_and_game_owned_presentation() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let out = temp.path().join("output");
        build_project(temp.path(), BuildTarget::Web, Some(&out)).unwrap();
        aria_core::CompiledProgram::decode(&fs::read(out.join("game.ariac")).unwrap()).unwrap();
        PakArchive::open(&fs::read(out.join("game.ariapak")).unwrap()).unwrap();
        assert!(out.join("service-worker.js").is_file());
        assert!(out.join("save-store.js").is_file());
        assert!(out.join("web-renderer.js").is_file());
        assert!(out.join("index.html").is_file());
        let bundle: BundleManifest =
            serde_json::from_slice(&fs::read(out.join("bundle.aria.json")).unwrap()).unwrap();
        assert_eq!(bundle.schema_version, 5);
        assert_eq!(bundle.content_root_blake3, bundle_content_root(&bundle));
        let manifest: BuildManifest =
            serde_json::from_slice(&fs::read(out.join("build-manifest.json")).unwrap()).unwrap();
        // C2: the web runtime auto-build completes the bundle, so no separate
        // runtime package is required. Hosts without the wasm toolchain fail
        // explicitly instead of reaching this assertion.
        assert!(!manifest.web_runtime_package_required);
        assert!(out.join("pkg/aria_web_bg.wasm").is_file());
        assert!(
            fs::read_to_string(out.join("index.html"))
                .unwrap()
                .contains("test presentation")
        );
    }

    #[test]
    fn web_presentation_includes_a_prebuilt_wasm_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("runtime");
        let out = temp.path().join("output");
        let frontend = temp.path().join("ui");
        fs::create_dir(&runtime).unwrap();
        fs::write(
            runtime.join("aria_web.js"),
            "export default async function() {}\nexport class WebRuntime {}\nexport class WebPak {}\n// aria_web_bg.wasm",
        )
        .unwrap();
        fs::write(runtime.join("aria_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
        fs::create_dir(&out).unwrap();
        write_test_presentation(temp.path());
        assert!(write_web_presentation(&out, Some(&runtime), &frontend).unwrap());
        assert!(out.join("pkg/aria_web.js").is_file());
        assert!(out.join("web-renderer.js").is_file());
        assert!(out.join("save-store.js").is_file());
        assert!(
            fs::read_to_string(out.join("index.html"))
                .unwrap()
                .contains("test presentation")
        );
    }

    #[test]
    fn declared_asset_roles_write_independent_packs() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let manifest = fs::read_to_string(temp.path().join("aria.toml")).unwrap();
        fs::write(
            temp.path().join("aria.toml"),
            manifest.replace(
                "fonts = []",
                "fonts = []\nasset_pack_roles = { \"assets/data.txt\" = \"cold\" }",
            ),
        )
        .unwrap();
        fs::write(temp.path().join("assets/boot.txt"), "boot").unwrap();
        let out = temp.path().join("output");
        build_project(temp.path(), BuildTarget::LinuxX64, Some(&out)).unwrap();

        let bundle: BundleManifest =
            serde_json::from_slice(&fs::read(out.join("bundle.aria.json")).unwrap()).unwrap();
        assert_eq!(bundle.pak_packs.len(), 2);
        assert_eq!(bundle.pak_packs[0].role, PakRole::Boot);
        assert_eq!(bundle.pak_packs[1].role, PakRole::Cold);
        assert!(out.join("game.ariapak").is_file());
        assert!(out.join("game.cold.ariapak").is_file());
        let cold = PakArchive::open(&fs::read(out.join("game.cold.ariapak")).unwrap()).unwrap();
        assert_eq!(cold.read("assets/data.txt").unwrap(), b"asset");
    }

    #[test]
    fn build_rejects_removed_compatibility_syntax() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        fs::write(
            temp.path().join("scripts/main.aria"),
            "# aria-version: 3.0\nstrict on\nui title, \"未実装\"\nend\n",
        )
        .unwrap();

        let dev_out = temp.path().join("dev-output");
        let dev_error = build_project(temp.path(), BuildTarget::Web, Some(&dev_out)).unwrap_err();
        assert!(dev_error.to_string().contains("compiler errors"));
        let release_out = temp.path().join("release-output");
        let error =
            build_project_with_release(temp.path(), BuildTarget::Web, Some(&release_out), true)
                .unwrap_err();
        assert!(error.to_string().contains("compiler errors"));
        assert!(!dev_out.exists());
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
            None,
            None,
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
