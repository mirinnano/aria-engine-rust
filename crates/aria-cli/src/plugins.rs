//! Finding and choosing `.ariaplugin` files for check, build, and run.
//!
//! Plugins are never listed by file. A host looks in its plugin folders in
//! order (the project or bundle `plugins/`, then the player's user-data
//! folder), verifies every file against the game's trusted keys, and keeps
//! the newest build of each plugin that the game's `[plugins] require`
//! range accepts.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aria_core::PluginRegistration;
use aria_core::project::PluginsManifest;
use aria_plugin_host::package::open_package;
use aria_plugin_host::select::{Candidate, Selection, select};

pub const PLUGIN_DIRECTORY: &str = "plugins";
pub const PLUGIN_EXTENSION: &str = "ariaplugin";

/// The user-data folder a Native Player also reads, so an update can be
/// installed without rewriting the game's own files.
#[cfg(feature = "desktop-player")]
#[must_use]
pub fn user_plugin_directory(game_id: &str) -> PathBuf {
    aria_native::default_save_root()
        .join(PLUGIN_DIRECTORY)
        .join(game_id)
}

/// Reads every `.ariaplugin` in `directories` (missing folders are skipped)
/// and selects the builds to run. Fails when a required plugin is missing.
pub fn discover(directories: &[PathBuf], manifest: &PluginsManifest) -> Result<Selection> {
    let trusted = manifest.trusted_key_bytes();
    let mut candidates = Vec::new();
    for directory in directories {
        for path in plugin_files(directory)? {
            let package = fs::read(&path)
                .with_context(|| format!("cannot read {}", path.display()))
                .map(|bytes| open_package(&bytes, &trusted));
            candidates.push(Candidate {
                source: path.display().to_string(),
                package: match package {
                    Ok(result) => result,
                    Err(error) => {
                        candidates_skip_io(&path, error);
                        continue;
                    }
                },
            });
        }
    }
    let require = manifest
        .require
        .iter()
        .map(|(id, range)| {
            semver::VersionReq::parse(range)
                .map(|range| (id.clone(), range))
                .with_context(|| format!("plugins.require.\"{id}\" is not a semver range"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let selection = select(candidates, &require);
    for line in &selection.skipped {
        eprintln!("aria: skipped plugin {line}");
    }
    if !selection.is_complete() {
        bail!(
            "required plugins are missing: {}",
            selection.missing.join(", ")
        );
    }
    Ok(selection)
}

/// The registrations and, when any plugin is called at run time, the Wasm
/// executor that a VM needs.
pub fn plugin_set(selection: &Selection) -> Result<aria_core::PluginSet> {
    let executor = aria_plugin_host::executor::WasmExecutor::new(
        selection
            .plugins
            .iter()
            .map(|package| (&package.registration.manifest, package.wasm.as_slice())),
    )?;
    Ok(aria_core::PluginSet {
        registrations: registrations(selection),
        executor: (!executor.is_empty()).then(|| {
            std::sync::Arc::new(executor) as std::sync::Arc<dyn aria_core::plugins::PluginExecutor>
        }),
    })
}

/// The registrations the VM needs, in plugin-id order.
#[must_use]
pub fn registrations(selection: &Selection) -> Vec<PluginRegistration> {
    selection
        .plugins
        .iter()
        .map(|package| package.registration.clone())
        .collect()
}

/// Copies the chosen packages into `<out>/plugins/` and returns their file
/// names, which a Web build lists in `plugins/index.json`.
pub fn copy_into_bundle(selection: &Selection, out: &Path) -> Result<Vec<String>> {
    let directory = out.join(PLUGIN_DIRECTORY);
    let mut names = Vec::new();
    for (package, source) in selection.plugins.iter().zip(&selection.sources) {
        let name = format!("{}.{PLUGIN_EXTENSION}", package.registration.manifest.id);
        fs::create_dir_all(&directory)?;
        fs::copy(source, directory.join(&name)).with_context(|| format!("cannot copy {source}"))?;
        names.push(name);
    }
    Ok(names)
}

/// `.ariaplugin` files directly inside `directory`, sorted by name.
pub fn plugin_files(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = fs::read_dir(directory)
        .with_context(|| format!("cannot list {}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == PLUGIN_EXTENSION)
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn candidates_skip_io(path: &Path, error: anyhow::Error) {
    eprintln!("aria: skipped plugin {}: {error:#}", path.display());
}
