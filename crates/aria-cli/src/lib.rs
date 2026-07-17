#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

pub mod build;
pub mod check;
pub mod migrate;
mod package_runtime;
#[cfg(all(feature = "desktop-player", not(target_arch = "wasm32")))]
pub mod player;
pub mod project;
pub mod release;
pub mod run;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

pub use build::{BuildProfile, BuildTarget};

#[derive(Debug, Parser)]
#[command(name = "aria", version, about = "AriaEngine V3 project toolchain")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse, analyze, and compile a V3 project without producing artifacts.
    Check {
        project: PathBuf,
        #[arg(long)]
        json: bool,
        /// Reject source that still needs a host compatibility command.
        #[arg(long)]
        release: bool,
    },
    /// Run a V3 project through the native runtime boundary.
    Run {
        project: PathBuf,
        #[arg(long)]
        headless: bool,
        #[arg(long)]
        replay: Option<PathBuf>,
        #[arg(long, default_value_t = 10_000)]
        max_frames: u64,
    },
    /// Build a target-specific player data bundle.
    Build {
        project: PathBuf,
        #[arg(long, value_enum)]
        target: BuildTarget,
        /// PAK4 distribution profile: dev, signed, or protected.
        #[arg(long, value_enum, default_value_t = BuildProfile::Dev)]
        profile: BuildProfile,
        /// Publisher Ed25519 key as `[key-id:]hex` for signed/protected packs.
        #[arg(long)]
        signing_key: Option<String>,
        /// XChaCha20-Poly1305 key as `[key-id:]hex` for protected packs.
        #[arg(long)]
        encryption_key: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Refuse to package V3 source containing unsupported host commands.
        #[arg(long)]
        release: bool,
    },
    /// Back up and migrate a V1/V2 project to the V3 project boundary.
    Migrate {
        project: PathBuf,
        #[arg(long)]
        game_id: Option<String>,
    },
}

pub fn run<I, T>(arguments: I) -> Result<u8>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::parse_from(arguments);
    match cli.command {
        Command::Check {
            project,
            json,
            release,
        } => check::command(&project, json, release),
        Command::Run {
            project,
            headless,
            replay,
            max_frames,
        } => {
            if headless || replay.is_some() {
                run::command(&project, true, replay.as_deref(), max_frames)
            } else {
                #[cfg(all(feature = "desktop-player", not(target_arch = "wasm32")))]
                {
                    player::run_project(&project)
                }
                #[cfg(any(not(feature = "desktop-player"), target_arch = "wasm32"))]
                {
                    run::command(&project, false, None, max_frames)
                }
            }
        }
        Command::Build {
            project,
            target,
            profile,
            signing_key,
            encryption_key,
            out,
            release,
        } => build::command_with_profile(
            &project,
            target,
            out.as_deref(),
            release,
            profile,
            signing_key.as_deref(),
            encryption_key.as_deref(),
        ),
        Command::Migrate { project, game_id } => migrate::command(&project, game_id.as_deref()),
    }
}
