//! Standalone validation of a Rust/Wasm plugin before project registration.
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Execute the bounded metadata entry point and inspect its validated API.
    Inspect {
        wasm: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub fn command(command: PluginCommand) -> Result<u8> {
    let PluginCommand::Inspect { wasm, json } = command;
    if fs::metadata(&wasm)
        .with_context(|| format!("cannot inspect {}", wasm.display()))?
        .len()
        > aria_plugin_host::MAX_PLUGIN_BYTES as u64
    {
        bail!("plugin exceeds the 4 MiB binary limit");
    }
    let plugin = aria_plugin_host::load_plugin(&fs::read(&wasm)?)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&plugin.manifest)?);
    } else {
        println!(
            "{} {} (API {})",
            plugin.manifest.id, plugin.manifest.version, plugin.manifest.api_version
        );
        println!("Wasm BLAKE3: {}", plugin.wasm_blake3);
        for route in plugin.manifest.routes {
            println!(
                "  {}: reading={}, atomic={}, advance={}",
                route.route,
                route.reading_surface,
                route.atomic_text,
                route.advance_action.as_deref().unwrap_or("authored hold")
            );
        }
    }
    Ok(0)
}
