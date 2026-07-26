#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

pub mod bench;
pub mod build;
pub mod check;
pub mod import_novel;
mod package;
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
pub use package::PackageFormat;

#[derive(Debug, Parser)]
#[command(name = "aria", version, about = "Aria project toolchain")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse, analyze, and compile an Aria project without producing artifacts.
    Check {
        project: PathBuf,
        #[arg(long)]
        json: bool,
        /// Validate release-only asset and bundled-font requirements.
        #[arg(long)]
        release: bool,
    },
    /// Run an Aria project through the native runtime boundary.
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
        /// Enable release validation and include a target-valid Player by default.
        #[arg(long)]
        release: bool,
        /// Automatically build the native Player binary for the target.
        /// Defaults to on when --release is set.
        #[arg(long)]
        build_player: Option<bool>,
        /// Path to a pre-built native Player binary (overrides auto-build and env).
        #[arg(long)]
        player: Option<PathBuf>,
        /// Override the manifest entry for a content-limited build edition.
        /// The value is a logical project path such as `scripts/main-demo.aria`.
        #[arg(long)]
        entry: Option<String>,
        /// Use a dedicated save namespace for a content-limited build edition.
        #[arg(long)]
        save_namespace: Option<String>,
    },
    /// Benchmark the VM hot loop with scripted advance/choice input.
    Bench {
        project: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        steps: u64,
        #[arg(long)]
        json: bool,
    },
    /// Convert canonical Markdown chapters into Aria, or verify an existing Aria import.
    ImportNovel {
        /// Directory containing the canonical Markdown chapter files.
        source: PathBuf,
        /// Aria library file to create, or output directory when --layout chapters is used.
        #[arg(long)]
        out: PathBuf,
        /// Scene used for the generated chapter catalogue.
        #[arg(long, default_value = "chapter_select_ja")]
        chapter_select: String,
        /// Locale applied when an imported chapter begins.
        #[arg(long, default_value = "ja-JP")]
        locale: String,
        /// Exact Markdown filenames to include (comma-separated; default: every chapter).
        #[arg(long, value_delimiter = ',')]
        include: Vec<String>,
        /// Optional project-owned presentation rules for the generated scenario.
        #[arg(long, value_enum, default_value_t = import_novel::NovelPresentation::Plain)]
        presentation: import_novel::NovelPresentation,
        /// Write index.aria plus one chapter-NN.aria file per selected chapter.
        #[arg(long, value_enum, default_value_t = import_novel::NovelImportLayout::Single)]
        layout: import_novel::NovelImportLayout,
        /// Validate existing output against Markdown without changing files.
        #[arg(long)]
        verify: bool,
    },
    /// Package a built bundle directory into a distributable zip (and NSIS installer on Windows).
    Package {
        /// Path to a built bundle directory (the output of `aria build`).
        bundle: PathBuf,
        /// Override the output directory (default: `dist` inside the bundle's parent).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Output policy: auto (portable + native installers), zip, installer, or web.
        #[arg(long, value_enum, default_value_t = PackageFormat::Auto)]
        format: PackageFormat,
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
            build_player,
            player,
            entry,
            save_namespace,
        } => build::command_with_profile_and_runtime_overrides(
            &project,
            target,
            out.as_deref(),
            release,
            profile,
            signing_key.as_deref(),
            encryption_key.as_deref(),
            build_player,
            player.as_deref(),
            entry.as_deref(),
            save_namespace.as_deref(),
        ),
        Command::Bench {
            project,
            steps,
            json,
        } => bench::command(&project, steps, json),
        Command::ImportNovel {
            source,
            out,
            chapter_select,
            locale,
            include,
            presentation,
            layout,
            verify,
        } => import_novel::command(
            &source,
            &out,
            import_novel::NovelImportOptions {
                chapter_select,
                locale,
                include,
                presentation,
                layout,
            },
            verify,
        ),
        Command::Package {
            bundle,
            out,
            format,
        } => package::command(&bundle, out.as_deref(), format),
    }
}
