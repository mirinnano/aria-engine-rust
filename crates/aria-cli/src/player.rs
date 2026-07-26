//! Graphical Native Player entry point used by `aria run` and packaged games.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use aria_core::pak::{PakArchive, PakError};
use aria_native::{
    AssetProvider, NativeAssetStore, NativePlayerConfig, default_save_root, run_desktop,
};
use aria_protection::PakPackage;
use clap::Parser;

use crate::project::AssetInventory;
use crate::run::{RuntimeAssetSource, load_runtime_project};

#[derive(Debug, Parser)]
#[command(name = "aria-player", about = "AriaEngine V3 Native Player")]
struct PlayerArgs {
    /// Project directory, aria.toml, or packaged Player directory.
    project: Option<PathBuf>,
    /// Use the deterministic terminal runner instead of opening a window.
    #[arg(long)]
    headless: bool,
    /// Replay a recorded deterministic input tape (implies headless mode).
    #[arg(long)]
    replay: Option<PathBuf>,
    #[arg(long, default_value_t = 10_000)]
    max_frames: u64,
}

/// Parses arguments for a packaged `aria-player` executable.
pub fn entry<I, T>(arguments: I) -> Result<u8>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args = PlayerArgs::parse_from(arguments);
    let project = args.project.unwrap_or_else(packaged_player_root);
    if args.headless || args.replay.is_some() {
        return crate::run::command(&project, true, args.replay.as_deref(), args.max_frames);
    }
    run_project(&project)
}

/// Launches the graphical Native Player for an explicit project/package.
pub fn run_project(path: &Path) -> Result<u8> {
    let project = load_runtime_project(path)?;
    let title = if project.title.trim().is_empty() {
        project.program.game_id.clone()
    } else {
        project.title.clone()
    };
    let assets: Box<dyn AssetProvider> = match project.asset_source {
        RuntimeAssetSource::ProjectRoot { assets } => {
            Box::new(DirectoryAssetProvider::new(project.root.clone(), assets)?)
        }
        RuntimeAssetSource::Package { profile, packs } => match profile {
            crate::build::BuildProfile::Dev => {
                let archives = packs
                    .iter()
                    .map(|pack| {
                        let path = project.root.join(&pack.file);
                        let bytes = fs::read(&path)
                            .with_context(|| format!("cannot read {}", path.display()))?;
                        PakArchive::open(&bytes).map_err(anyhow::Error::from)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Box::new(PakAssetProvider {
                    kind: PakAssetProviderKind::Core(archives),
                })
            }
            crate::build::BuildProfile::Signed | crate::build::BuildProfile::Protected => {
                let keys = crate::build::resolve_pak_keys(profile, None, None)?;
                let mut provider = aria_protection::StaticPakKeyProvider::new();
                if let Some(key) = keys.signing.as_ref() {
                    provider = provider.with_signing_key(key);
                }
                if let Some(key) = keys.encryption.as_ref() {
                    provider = provider.with_encryption_key(key);
                }
                let packages = packs
                    .iter()
                    .map(|pack| {
                        let path = project.root.join(&pack.file);
                        let bytes = fs::read(&path)
                            .with_context(|| format!("cannot read {}", path.display()))?;
                        PakPackage::open(&bytes, Some(&provider)).map_err(anyhow::Error::from)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Box::new(PakAssetProvider {
                    kind: PakAssetProviderKind::Protected(packages),
                })
            }
        },
    };
    run_desktop(NativePlayerConfig {
        title,
        program: project.program,
        logical_size: project.logical_size,
        save_root: default_save_root(),
        save_namespace: project.save_namespace,
        legacy_save_namespaces: project.legacy_save_namespaces,
        font_assets: project.font_assets,
        assets: NativeAssetStore::new(assets),
    })?;
    Ok(0)
}

fn packaged_player_root() -> PathBuf {
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_owned))
        .unwrap_or_else(|| PathBuf::from("."));
    // Native macOS apps keep the executable in Contents/MacOS and the Aria
    // bundle in Contents/Resources. Other targets continue to use the
    // executable's directory exactly as before.
    if executable_dir
        .file_name()
        .is_some_and(|name| name == "MacOS")
        && let Some(contents) = executable_dir.parent()
    {
        let resources = contents.join("Resources");
        if resources.is_dir() {
            return resources;
        }
    }
    executable_dir
}

#[derive(Debug)]
struct DirectoryAssetProvider {
    root: PathBuf,
    assets: AssetInventory,
}

impl DirectoryAssetProvider {
    fn new(root: PathBuf, assets: AssetInventory) -> Result<Self> {
        Ok(Self {
            root: root
                .canonicalize()
                .with_context(|| format!("cannot resolve asset root {}", root.display()))?,
            assets,
        })
    }
}

impl AssetProvider for DirectoryAssetProvider {
    fn read_asset(&mut self, logical_path: &str) -> Result<Vec<u8>, String> {
        let logical = aria_core::compiler::normalize_logical_path(logical_path)?;
        if logical != logical_path {
            return Err(format!(
                "asset '{logical_path}' is not a canonical NFC '/' logical path; use '{logical}'"
            ));
        }
        if !self.assets.contains(&logical) {
            return Err(format!(
                "asset '{logical}' is not declared by runtime.asset_roots or differs in case/Unicode spelling"
            ));
        }
        let path = logical
            .split('/')
            .fold(self.root.clone(), |path, component| path.join(component));
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("cannot resolve asset '{logical}': {error}"))?;
        if !canonical.starts_with(&self.root) {
            return Err(format!(
                "asset '{logical}' resolves outside the project root"
            ));
        }
        fs::read(&canonical).map_err(|error| format!("cannot read asset '{logical}': {error}"))
    }
}

#[derive(Debug)]
struct PakAssetProvider {
    kind: PakAssetProviderKind,
}

#[derive(Debug)]
enum PakAssetProviderKind {
    Core(Vec<PakArchive>),
    Protected(Vec<PakPackage>),
}

impl AssetProvider for PakAssetProvider {
    fn read_asset(&mut self, logical_path: &str) -> Result<Vec<u8>, String> {
        match &self.kind {
            PakAssetProviderKind::Core(archives) => {
                let mut last_error = None;
                for archive in archives {
                    match archive.read(logical_path) {
                        Ok(bytes) => return Ok(bytes),
                        Err(error @ PakError::MissingAsset(_)) => last_error = Some(error),
                        Err(error) => return Err(error.to_string()),
                    }
                }
                Err(last_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| format!("missing asset '{logical_path}'")))
            }
            PakAssetProviderKind::Protected(packages) => {
                let mut last_error = None;
                for package in packages {
                    match package.read(logical_path) {
                        Ok(bytes) => return Ok(bytes),
                        Err(
                            error @ aria_protection::ProtectionError::Inner(PakError::MissingAsset(
                                _,
                            )),
                        ) => last_error = Some(error),
                        Err(error) => return Err(error.to_string()),
                    }
                }
                Err(last_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| format!("missing asset '{logical_path}'")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loose_asset_provider_rejects_a_path_that_escapes_the_project() {
        let temp = tempfile::tempdir().unwrap();
        let mut provider =
            DirectoryAssetProvider::new(temp.path().to_owned(), AssetInventory::empty()).unwrap();
        assert!(provider.read_asset("../../secret.png").is_err());
    }
}
