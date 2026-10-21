//! `aria plugin`: build a signed `.ariaplugin`, and inspect one.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aria_plugin_host::package::{UI_MODULE_FILE, open_package, pack};
use clap::Subcommand;
use walkdir::WalkDir;

use crate::project::LoadedProject;

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Sign a Wasm plugin (and optional extras) into one `.ariaplugin` file.
    Pack {
        /// The plugin built for `wasm32-unknown-unknown`.
        wasm: PathBuf,
        /// Output file, normally `<project>/plugins/<id>.ariaplugin`.
        #[arg(long)]
        out: PathBuf,
        /// A Web UI module (ES module) to ship as `ui.js`.
        #[arg(long)]
        ui: Option<PathBuf>,
        /// A directory whose files ship under `assets/`.
        #[arg(long)]
        assets: Option<PathBuf>,
        /// `key-id:64-hex-private-key`. Defaults to `ARIA_PAK_SIGNING_KEY`.
        #[arg(long)]
        signing_key: Option<String>,
    },
    /// Show what a `.wasm` or `.ariaplugin` declares.
    Inspect {
        file: PathBuf,
        /// Project whose `[plugins] trusted_keys` verify an `.ariaplugin`.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub fn command(command: PluginCommand) -> Result<u8> {
    match command {
        PluginCommand::Pack {
            wasm,
            out,
            ui,
            assets,
            signing_key,
        } => pack_command(&wasm, &out, ui.as_deref(), assets.as_deref(), signing_key),
        PluginCommand::Inspect {
            file,
            project,
            json,
        } => inspect_command(&file, &project, json),
    }
}

fn pack_command(
    wasm: &Path,
    out: &Path,
    ui: Option<&Path>,
    assets: Option<&Path>,
    signing_key: Option<String>,
) -> Result<u8> {
    let key = match signing_key {
        Some(key) => key,
        None => std::env::var("ARIA_PAK_SIGNING_KEY")
            .context("pass --signing-key or set ARIA_PAK_SIGNING_KEY")?,
    };
    let (key_id, secret) = parse_signing_key(&key)?;
    let mut extra = BTreeMap::new();
    if let Some(ui) = ui {
        extra.insert(
            UI_MODULE_FILE.to_owned(),
            fs::read(ui).with_context(|| format!("cannot read {}", ui.display()))?,
        );
    }
    if let Some(assets) = assets {
        for entry in WalkDir::new(assets).follow_links(false).sort_by_file_name() {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry.path().strip_prefix(assets)?;
            let name = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            extra.insert(format!("assets/{name}"), fs::read(entry.path())?);
        }
    }
    let wasm_bytes = fs::read(wasm).with_context(|| format!("cannot read {}", wasm.display()))?;
    let bytes = pack(&wasm_bytes, &extra, &key_id, &secret)?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, &bytes).with_context(|| format!("cannot write {}", out.display()))?;
    let manifest = aria_plugin_host::load_plugin(&wasm_bytes)?.manifest;
    println!(
        "{} {} → {} ({} bytes, signed by '{key_id}')",
        manifest.id,
        manifest.version,
        out.display(),
        bytes.len()
    );
    Ok(0)
}

fn inspect_command(file: &Path, project: &Path, json: bool) -> Result<u8> {
    let bytes = fs::read(file).with_context(|| format!("cannot inspect {}", file.display()))?;
    let (registration, signer, extras) = if file
        .extension()
        .is_some_and(|extension| extension == "ariaplugin")
    {
        let project = LoadedProject::load(project).with_context(|| {
            format!(
                "an .ariaplugin is verified with the trusted keys in {}/aria.toml",
                project.display()
            )
        })?;
        let package = open_package(&bytes, &project.manifest.plugins.trusted_key_bytes())?;
        let extras = package.files.keys().cloned().collect::<Vec<_>>();
        (package.registration, Some(package.key_id), extras)
    } else {
        if bytes.len() > aria_plugin_host::MAX_PLUGIN_BYTES {
            bail!("plugin exceeds the 4 MiB binary limit");
        }
        (aria_plugin_host::load_plugin(&bytes)?, None, Vec::new())
    };
    let manifest = registration.manifest;
    if json {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(0);
    }
    println!(
        "{} {} (API {}, state version {})",
        manifest.id, manifest.version, manifest.api_version, manifest.state_version
    );
    if let Some(signer) = signer {
        println!("signed by: {signer}");
    }
    println!("Wasm BLAKE3: {}", registration.wasm_blake3);
    let capabilities = manifest
        .capabilities
        .iter()
        .map(|capability| format!("{capability:?}").to_lowercase())
        .collect::<Vec<_>>();
    println!("capabilities: {}", capabilities.join(", "));
    for route in manifest.routes {
        println!(
            "  route {}: reading={}, atomic={}, advance={}",
            route.route,
            route.reading_surface,
            route.atomic_text,
            route.advance_action.as_deref().unwrap_or("authored hold")
        );
    }
    for name in extras {
        println!("  file {name}");
    }
    Ok(0)
}

fn parse_signing_key(value: &str) -> Result<(String, [u8; 32])> {
    let (id, hex) = value.split_once(':').unwrap_or(("publisher", value));
    let bytes: [u8; 32] = hex::decode(hex.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .context("the signing key must be 32 bytes in 64 hexadecimal characters")?;
    Ok((id.to_owned(), bytes))
}
