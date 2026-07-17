use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aes::Aes256;
use anyhow::{Context, Result, bail};
use aria_core::bytecode::{ByteOp, Constant, Operand};
use aria_core::compiler::{CompileInput, SourceUnit, normalize_logical_path};
use aria_core::migration::migrate_legacy_config;
use aria_core::modern::parse as parse_modern;
use aria_core::pak::{AssetInput, PakArchive};
use aria_core::project::{GameManifest, ProjectManifest, RuntimeManifest};
use aria_core::syntax::{SyntaxKind, SyntaxTree, unquote};
use aria_core::{CompileOutput, SaveEnvelopeV3, Severity, compile};
use atomic_write_file::AtomicWriteFile;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

use crate::project::LoadedProject;

type Aes256CbcDecoder = cbc::Decryptor<Aes256>;
type StagedSaves = (usize, Vec<String>, Vec<(PathBuf, Vec<u8>)>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationReport {
    pub schema_version: u32,
    pub already_v3: bool,
    pub backup_directory: Option<String>,
    pub scripts_migrated: usize,
    /// Legacy sources that deliberately were not rewritten because no
    /// behavior-preserving Aria 3.1 lowering exists yet.  This is separate
    /// from compiler diagnostics so automation never mistakes the retired
    /// 3.0 compatibility path for a successful modern-language migration.
    #[serde(default)]
    pub scripts_failed: Vec<String>,
    pub config_migrated: bool,
    pub saves_migrated: usize,
    pub saves_failed: Vec<String>,
    pub legacy_paks_backed_up: usize,
    pub v3_pak_built: bool,
    pub compiler_errors_after_migration: usize,
    pub compiler_warnings_after_migration: usize,
    pub notices: Vec<String>,
}

pub fn command(path: &Path, game_id: Option<&str>) -> Result<u8> {
    let report = migrate_project(path, game_id, now_unix_ms())?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(
        if report.saves_failed.is_empty()
            && report.scripts_failed.is_empty()
            && report.compiler_errors_after_migration == 0
        {
            0
        } else {
            2
        },
    )
}

pub fn migrate_project(
    path: &Path,
    requested_game_id: Option<&str>,
    timestamp_unix_ms: u64,
) -> Result<MigrationReport> {
    let root = path
        .canonicalize()
        .with_context(|| format!("cannot resolve legacy project {}", path.display()))?;
    if !root.is_dir() {
        bail!("legacy project is not a directory: {}", root.display());
    }
    let manifest_path = root.join("aria.toml");
    if manifest_path.is_file()
        && fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|source| ProjectManifest::from_toml(&source).ok())
            .is_some()
    {
        // Idempotent does not mean unverified. A first migration can create a
        // V3 manifest before the converted source compiles; treating every
        // later invocation as a success would hide that failure forever.
        let (compiler_errors_after_migration, compiler_warnings_after_migration, diagnostics) =
            compile_diagnostics(&root)?;
        let scripts_failed = modern_source_failures(&root, &manifest_path)?;
        let mut notices =
            vec!["project already has a valid V3 aria.toml; revalidated its source".to_owned()];
        notices.extend(diagnostics);
        if !scripts_failed.is_empty() {
            notices.push(
                "one or more sources are not valid Aria 3.1; the alpha 3.0 bridge is not a migration success"
                    .to_owned(),
            );
        }
        let report = MigrationReport {
            schema_version: 4,
            already_v3: true,
            backup_directory: None,
            scripts_migrated: 0,
            scripts_failed,
            config_migrated: false,
            saves_migrated: 0,
            saves_failed: Vec::new(),
            legacy_paks_backed_up: 0,
            v3_pak_built: false,
            compiler_errors_after_migration,
            compiler_warnings_after_migration,
            notices,
        };
        write_atomic(
            &root.join("aria-migrate-report.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        return Ok(report);
    }

    let backup = root
        .join(".aria-migrate-backup")
        .join(timestamp_unix_ms.to_string());
    backup_inputs(&root, &backup)?;

    let init = infer_legacy_init(&root)?;
    let game_id = requested_game_id
        .map(str::to_owned)
        .unwrap_or_else(|| inferred_game_id(&root));
    // The manifest always uses the portable `assets` root.  Do not create it
    // during preflight: a failed conversion must not leave a new directory in
    // a legacy project.  The commit phase below creates it only after every
    // read/parse/pack operation has succeeded.
    let asset_root = "assets";
    let manifest = ProjectManifest {
        schema: 3,
        game: GameManifest {
            id: game_id.clone(),
            version: "1.0.0".to_owned(),
            title: init.title,
        },
        runtime: RuntimeManifest {
            entry: init.entry.clone(),
            logical_width: init.width,
            logical_height: init.height,
            asset_roots: vec![asset_root.to_owned()],
            fonts: init.fonts,
            save_namespace: game_id.replace('.', "_"),
        },
    };
    let manifest_source = manifest
        .to_toml()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let script_migration = migrate_scripts_to_modern(&root, &init.entry, &game_id)?;
    let mut notices = script_migration.notices;
    if !script_migration.failures.is_empty() {
        notices.push(
            "no project source, config, save, manifest, or pak was rewritten because Aria 3.1 conversion is incomplete"
                .to_owned(),
        );
        notices.sort();
        notices.dedup();
        let report = MigrationReport {
            schema_version: 4,
            already_v3: false,
            backup_directory: Some(relative_display(&root, &backup)),
            scripts_migrated: 0,
            scripts_failed: script_migration.failures,
            config_migrated: false,
            saves_migrated: 0,
            saves_failed: Vec::new(),
            legacy_paks_backed_up: 0,
            v3_pak_built: false,
            compiler_errors_after_migration: 0,
            compiler_warnings_after_migration: 0,
            notices,
        };
        write_atomic(
            &root.join("aria-migrate-report.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        return Ok(report);
    }
    let scripts_migrated = script_migration.scripts_migrated;
    // Preflight every operation that can fail before changing the source
    // graph. Unsupported source conversion already returned above; this
    // second staging pass also keeps malformed config, save, or pak input from
    // leaving a half-written migration behind.
    let config_path = root.join("config.json");
    let migrated_config = if config_path.is_file() {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&config_path)?)?;
        Some(serde_json::to_vec_pretty(&migrate_legacy_config(&value))?)
    } else {
        None
    };
    let (saves_migrated, saves_failed, staged_saves) =
        migrate_saves(&root, &game_id, timestamp_unix_ms)?;
    let legacy_paks_backed_up = project_files(&root, |entry| {
        entry
            .path()
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pak"))
    })?
    .len();
    let staged_pak = rebuild_assets(&root, &game_id)?;
    let staged_compile = compile_staged_sources(&root, &manifest, &script_migration.scripts)?;
    let staged_asset_failures = staged_program_asset_failures(&root, &manifest, &staged_compile);
    if staged_compile.has_errors() || !staged_asset_failures.is_empty() {
        let compiler_errors_after_migration = staged_compile
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .count();
        let compiler_warnings_after_migration = staged_compile
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Warning)
            .count();
        let mut failures = staged_asset_failures;
        failures.extend(
            staged_compile
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == Severity::Error)
                .map(ToString::to_string),
        );
        notices.push(
            "no project source, config, save, manifest, or pak was rewritten because the staged Aria 3.1 project did not compile cleanly"
                .to_owned(),
        );
        notices.extend(
            staged_compile
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == Severity::Warning)
                .map(ToString::to_string),
        );
        notices.sort();
        notices.dedup();
        let report = MigrationReport {
            schema_version: 4,
            already_v3: false,
            backup_directory: Some(relative_display(&root, &backup)),
            scripts_migrated: 0,
            scripts_failed: failures,
            config_migrated: false,
            saves_migrated: 0,
            saves_failed: Vec::new(),
            legacy_paks_backed_up: 0,
            v3_pak_built: false,
            compiler_errors_after_migration,
            compiler_warnings_after_migration,
            notices,
        };
        write_atomic(
            &root.join("aria-migrate-report.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        return Ok(report);
    }

    for script in script_migration.scripts {
        if script.changed {
            write_atomic(&script.path, script.source.as_bytes())?;
        }
    }

    let config_migrated = if let Some(config) = migrated_config {
        write_atomic(&root.join("user-config.v3.json"), &config)?;
        true
    } else {
        false
    };

    for (path, bytes) in staged_saves {
        write_atomic(&path, &bytes)?;
    }
    let v3_pak_built = if let Some(pak) = staged_pak {
        let output = root.join("v3-data");
        fs::create_dir_all(&output)?;
        write_atomic(&output.join("game.ariapak"), &pak)?;
        true
    } else {
        false
    };
    fs::create_dir_all(root.join("assets"))?;
    write_atomic(&manifest_path, manifest_source.as_bytes())?;

    notices.sort();
    notices.dedup();
    if legacy_paks_backed_up > 0 {
        notices.push(
            "legacy pak files were retained only in the backup; V3 pak was rebuilt from unpacked assets"
                .to_owned(),
        );
    }

    let (compiler_errors_after_migration, compiler_warnings_after_migration, diagnostics) =
        compile_diagnostics(&root)?;
    notices.extend(diagnostics);
    let scripts_failed = modern_source_failures(&root, &manifest_path)?;

    let report = MigrationReport {
        schema_version: 4,
        already_v3: false,
        backup_directory: Some(relative_display(&root, &backup)),
        scripts_migrated,
        scripts_failed,
        config_migrated,
        saves_migrated,
        saves_failed,
        legacy_paks_backed_up,
        v3_pak_built,
        compiler_errors_after_migration,
        compiler_warnings_after_migration,
        notices,
    };
    write_atomic(
        &root.join("aria-migrate-report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn compile_diagnostics(root: &Path) -> Result<(usize, usize, Vec<String>)> {
    let loaded = LoadedProject::load(root)?;
    let compiled = loaded.compile()?;
    let compiler_errors = compiled
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count();
    let compiler_warnings = compiled
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    Ok((
        compiler_errors,
        compiler_warnings,
        compiled
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect(),
    ))
}

fn compile_staged_sources(
    root: &Path,
    manifest: &ProjectManifest,
    scripts: &[ConvertedScript],
) -> Result<CompileOutput> {
    let sources = scripts
        .iter()
        .map(|script| {
            Ok(SourceUnit {
                logical_path: crate::project::logical_path(root, &script.path)?,
                source: script.source.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(compile(CompileInput {
        game_id: manifest.game.id.clone(),
        entry: manifest.runtime.entry.clone(),
        sources,
    }))
}

fn staged_program_asset_failures(
    root: &Path,
    manifest: &ProjectManifest,
    compiled: &CompileOutput,
) -> Vec<String> {
    let Some(program) = &compiled.program else {
        return Vec::new();
    };
    let mut failures = Vec::new();
    for (instruction_index, instruction) in program.instructions.iter().enumerate() {
        let (operand_index, accepts_color, kind) = match instruction.op {
            ByteOp::Background => (0, true, "background"),
            ByteOp::SpriteImage => (1, false, "image"),
            ByteOp::PlayAudio => (2, false, "audio"),
            _ => continue,
        };
        let Some(Operand::Constant(constant)) = instruction.operands.get(operand_index) else {
            continue;
        };
        let Some(Constant::String(path)) = program.constants.get(*constant as usize) else {
            continue;
        };
        if accepts_color && path.starts_with('#') {
            continue;
        }
        let source_location = program.source_map.get(instruction_index);
        let location = source_location.map_or_else(
            || "<generated>:1:1".to_owned(),
            |location| format!("{}:{}:{}", location.source, location.line, location.column),
        );
        let Ok(canonical) = normalize_logical_path(path) else {
            failures.push(format!(
                "{location}: {kind} asset '{path}' is not a valid logical path"
            ));
            continue;
        };
        if canonical != *path
            || !manifest.runtime.asset_roots.iter().any(|asset_root| {
                canonical
                    .strip_prefix(asset_root)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            })
        {
            failures.push(format!(
                "{location}: {kind} asset '{path}' is outside runtime.asset_roots or is not canonical"
            ));
            continue;
        }
        let disk_path = canonical
            .split('/')
            .fold(root.to_owned(), |path, component| path.join(component));
        match fs::symlink_metadata(&disk_path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => failures.push(format!(
                "{location}: {kind} asset '{path}' is not a regular file"
            )),
            Err(_) => failures.push(format!(
                "{location}: {kind} asset '{path}' is missing from the migrated project"
            )),
        }
    }
    failures
}

#[derive(Debug)]
struct LegacyInit {
    title: String,
    entry: String,
    width: u32,
    height: u32,
    fonts: Vec<String>,
}

fn infer_legacy_init(root: &Path) -> Result<LegacyInit> {
    let path = root.join("init.aria");
    let source = if path.is_file() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let tree = SyntaxTree::parse("init.aria", source);
    let mut init = LegacyInit {
        title: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Aria V3 Game")
            .to_owned(),
        entry: "assets/scripts/main.aria".to_owned(),
        width: 1280,
        height: 720,
        fonts: Vec::new(),
    };
    for line in tree.lines {
        let SyntaxKind::Command(command) = line.kind else {
            continue;
        };
        match command.name.as_str() {
            "script" => {
                if let Some(entry) = command.arguments.first() {
                    init.entry = unquote(entry).replace('\\', "/");
                }
            }
            "window" => {
                if let Some(width) = command
                    .arguments
                    .first()
                    .and_then(|value| value.parse().ok())
                {
                    init.width = width;
                }
                if let Some(height) = command
                    .arguments
                    .get(1)
                    .and_then(|value| value.parse().ok())
                {
                    init.height = height;
                }
                if let Some(title) = command.arguments.get(2) {
                    init.title = unquote(title);
                }
            }
            "caption" => {
                if let Some(title) = command.arguments.first() {
                    init.title = unquote(title);
                }
            }
            "font" => {
                if let Some(font) = command.arguments.first() {
                    let font = unquote(font);
                    if let Ok(font) = aria_core::compiler::normalize_logical_path(&font)
                        && root.join(&font).is_file()
                        && !init.fonts.contains(&font)
                    {
                        init.fonts.push(font);
                    }
                }
            }
            _ => {}
        }
    }
    if !root.join(&init.entry).is_file() {
        if root.join("main.aria").is_file() {
            init.entry = "main.aria".to_owned();
        } else {
            bail!("could not locate legacy entry script '{}'", init.entry);
        }
    }
    Ok(init)
}

#[derive(Debug)]
struct ConvertedScript {
    path: PathBuf,
    source: String,
    changed: bool,
}

#[derive(Debug)]
struct ScriptMigrationBatch {
    scripts: Vec<ConvertedScript>,
    scripts_migrated: usize,
    failures: Vec<String>,
    notices: Vec<String>,
}

/// Parses every legacy script into the small, explicit Aria 3.1 subset before
/// writing any file. This all-or-nothing boundary is important: a project
/// must never contain a half-converted import graph that happens to compile
/// on one host.
fn migrate_scripts_to_modern(
    root: &Path,
    entry: &str,
    _game_id: &str,
) -> Result<ScriptMigrationBatch> {
    let entry = normalize_logical_path(entry).map_err(|error| anyhow::anyhow!(error))?;
    let entry_path = root.join(entry.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !entry_path.is_file() {
        bail!("entry script is missing: {}", entry_path.display());
    }

    let mut files = Vec::new();
    for path in project_files(root, |entry| {
        entry.path().extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("aria")
                && entry.file_name() != std::ffi::OsStr::new("init.aria")
        })
    })? {
        let logical = crate::project::logical_path(root, &path)?;
        files.push((path, logical));
    }
    files.sort_by(|left, right| left.1.cmp(&right.1));

    let mut failures = Vec::new();
    let mut notices = Vec::new();
    let mut converted = Vec::new();
    let mut seen_scene_names = BTreeSet::new();
    let mut entry_scene = None;

    for (path, logical) in &files {
        let source = fs::read_to_string(path)
            .with_context(|| format!("cannot read legacy source {}", path.display()))?;
        match convert_legacy_source(&source, logical, "assets") {
            Ok(mut result) => {
                for scene in &result.scene_names {
                    if !seen_scene_names.insert(scene.clone()) {
                        failures.push(format!(
                            "{logical}: scene '{scene}' is duplicated across the import graph"
                        ));
                    }
                }
                if logical == &entry {
                    entry_scene = result.entry_scene.clone();
                }
                result.body.shrink_to_fit();
                converted.push((path.clone(), logical.clone(), result));
            }
            Err(error) => failures.push(format!("{logical}: {error}")),
        }
    }

    if !files.iter().any(|(_, logical)| logical == &entry) {
        failures.push(format!("entry source '{entry}' was not found"));
    }
    if entry_scene.is_none() {
        failures.push(format!("entry source '{entry}' contains no scene"));
    }

    if !failures.is_empty() {
        return Ok(ScriptMigrationBatch {
            scripts: Vec::new(),
            scripts_migrated: 0,
            failures,
            notices,
        });
    }

    let mut scripts = Vec::new();
    let mut scripts_migrated = 0;
    let import_paths = files
        .iter()
        .filter(|(_, logical)| logical != &entry)
        .map(|(_, logical)| logical.clone())
        .collect::<Vec<_>>();
    let entry_scene = entry_scene.expect("checked above");
    for (path, logical, mut result) in converted {
        if logical == entry {
            let mut header = format!("aria 3.1;\nentry {entry_scene};\n");
            for imported in &import_paths {
                header.push_str(&format!(
                    "import {};\n",
                    quote_modern(&relative_import_path(&entry, imported))
                ));
            }
            header.push('\n');
            header.push_str(&result.body);
            result.body = header;
        } else {
            result.body = format!("aria 3.1;\n\n{}", result.body);
        }
        let original = fs::read_to_string(&path)?;
        let changed = original != result.body;
        if changed {
            scripts_migrated += 1;
        }
        scripts.push(ConvertedScript {
            path,
            source: result.body,
            changed,
        });
    }
    notices.push(format!(
        "converted {scripts_migrated} legacy source(s) to structured Aria 3.1"
    ));
    Ok(ScriptMigrationBatch {
        scripts,
        scripts_migrated,
        failures,
        notices,
    })
}

#[derive(Debug)]
struct ConvertedSource {
    body: String,
    scene_names: Vec<String>,
    entry_scene: Option<String>,
}

fn convert_legacy_source(
    source: &str,
    logical_path: &str,
    asset_root: &str,
) -> Result<ConvertedSource> {
    let tree = SyntaxTree::parse(logical_path, source);
    if tree
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        bail!("source syntax is malformed");
    }
    let mut labels = Vec::new();
    for line in &tree.lines {
        if let SyntaxKind::Label(label) = &line.kind {
            let scene = modern_identifier(label)?;
            if labels.contains(&scene) {
                bail!("duplicate label '{label}'");
            }
            labels.push(scene);
        }
    }
    if labels.is_empty() {
        labels.push("start".to_owned());
    }
    let entry_scene = labels.first().cloned();

    let mut body = String::new();
    let mut current_scene = None::<usize>;
    let mut terminated = false;
    for line in &tree.lines {
        match &line.kind {
            SyntaxKind::Empty | SyntaxKind::Directive { .. } => {}
            SyntaxKind::Comment => {
                if current_scene.is_some() {
                    body.push_str("// ");
                    body.push_str(line.raw.trim());
                    body.push('\n');
                }
            }
            SyntaxKind::Label(label) => {
                if let Some(previous) = current_scene {
                    if !terminated {
                        bail!(
                            "scene '{}' falls through before label '{label}'",
                            labels[previous]
                        );
                    }
                    body.push_str("}\n\n");
                }
                let scene = modern_identifier(label)?;
                let index = labels
                    .iter()
                    .position(|value| value == &scene)
                    .expect("label was collected above");
                body.push_str(&format!("scene {scene} {{\n"));
                current_scene = Some(index);
                terminated = false;
            }
            SyntaxKind::Dialogue { speaker, content } => {
                ensure_scene(&mut body, &mut current_scene, &labels)?;
                ensure_not_terminated(terminated, line.line)?;
                let text = quote_modern(content);
                if let Some(speaker) = speaker {
                    body.push_str(&format!("  say {}: {text};\n", quote_identifier(speaker)?));
                } else {
                    body.push_str(&format!("  narrate {text};\n"));
                }
                body.push_str("  await advance;\n");
            }
            SyntaxKind::Assignment { .. } => {
                bail!(
                    "line {} uses a legacy assignment form; migrate the target and declaration explicitly",
                    line.line
                );
            }
            SyntaxKind::Advance { clear_page } => {
                ensure_scene(&mut body, &mut current_scene, &labels)?;
                ensure_not_terminated(terminated, line.line)?;
                body.push_str("  await advance;\n");
                if *clear_page {
                    body.push_str("  clear dialogue;\n");
                }
            }
            SyntaxKind::Command(command) => {
                ensure_scene(&mut body, &mut current_scene, &labels)?;
                ensure_not_terminated(terminated, line.line)?;
                let (lines, is_terminal) = convert_legacy_command(command, asset_root, &labels)?;
                for converted_line in lines {
                    body.push_str("  ");
                    body.push_str(&converted_line);
                    body.push('\n');
                }
                terminated = is_terminal;
            }
        }
    }
    if current_scene.is_none() {
        bail!("source contains no executable scene");
    }
    if !terminated {
        bail!(
            "scene '{}' falls through; add an explicit end/return/jump",
            labels.last().unwrap()
        );
    }
    body.push_str("}\n");
    Ok(ConvertedSource {
        body,
        scene_names: labels,
        entry_scene,
    })
}

fn ensure_scene(
    body: &mut String,
    current_scene: &mut Option<usize>,
    labels: &[String],
) -> Result<()> {
    if current_scene.is_none() {
        body.push_str(&format!("scene {} {{\n", labels[0]));
        *current_scene = Some(0);
    }
    Ok(())
}

fn ensure_not_terminated(terminated: bool, line: u32) -> Result<()> {
    if terminated {
        bail!("line {line} is unreachable after a terminal control transfer");
    }
    Ok(())
}

fn convert_legacy_command(
    command: &aria_core::syntax::CommandSyntax,
    asset_root: &str,
    labels: &[String],
) -> Result<(Vec<String>, bool)> {
    let name = command.name.as_str();
    let args = &command.arguments;
    let one = |message: &str| {
        args.first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!(message.to_owned()))
    };
    match name {
        "end" | "quit" => Ok((vec!["end;".to_owned()], true)),
        "return" => Ok((vec!["return;".to_owned()], true)),
        "goto" | "jmp" => {
            let target = label_target(&one("goto requires a label")?, labels)?;
            Ok((vec![format!("jump {target};")], true))
        }
        "gosub" => {
            let target = label_target(&one("gosub requires a label")?, labels)?;
            Ok((vec![format!("call {target};")], false))
        }
        "choice" => {
            if args.len() < 2 || !args.len().is_multiple_of(2) {
                bail!("choice requires text/label pairs");
            }
            let mut lines = vec!["choice {".to_owned()];
            for pair in args.chunks_exact(2) {
                let target = label_target(&pair[1], labels)?;
                lines.push(format!("{} => {target};", quote_modern(&unquote(&pair[0]))));
            }
            lines.push("}".to_owned());
            Ok((lines, true))
        }
        "text" => Ok((
            vec![format!(
                "narrate {};",
                quote_modern(&unquote(&one("text requires a string")?))
            )],
            false,
        )),
        "textclear" | "erasetextwindow" => Ok((vec!["clear dialogue;".to_owned()], false)),
        "waitclick" | "wait_click" => Ok((vec!["await advance;".to_owned()], false)),
        "wait" => {
            let value = parse_u32(&one("wait requires milliseconds")?)?;
            Ok((vec![format!("wait {value}ms;")], false))
        }
        "bg" | "loadbg" | "load_bg" => {
            let path =
                legacy_asset_path(&unquote(&one("background requires an asset")?), asset_root);
            let duration = args.get(1).map(|value| parse_u32(value)).transpose()?;
            let statement = duration.map_or_else(
                || format!("background asset({});", quote_modern(&path)),
                |duration| {
                    format!(
                        "background asset({}) with fade({duration}ms);",
                        quote_modern(&path)
                    )
                },
            );
            Ok((vec![statement], false))
        }
        "lsp" | "loadch" | "load_ch" => {
            if args.len() < 4 {
                bail!("lsp requires id, asset, x, and y");
            }
            let id = sprite_identifier(&args[0])?;
            let path = legacy_asset_path(&unquote(&args[1]), asset_root);
            let x = parse_i32(&args[2])?;
            let y = parse_i32(&args[3])?;
            Ok((
                vec![format!(
                    "show {id} = image(asset({})) at ({x}px, {y}px) z 0;",
                    quote_modern(&path)
                )],
                false,
            ))
        }
        "msp" | "charmove" | "char_move" => {
            if args.len() < 3 {
                bail!("msp requires id, x, and y");
            }
            let id = sprite_identifier(&args[0])?;
            let x = parse_i32(&args[1])?;
            let y = parse_i32(&args[2])?;
            Ok((vec![format!("move {id} to ({x}px, {y}px);")], false))
        }
        "csp" | "clr" | "hidech" | "hide_ch" => {
            let id = sprite_identifier(&one("remove requires a sprite id")?)?;
            Ok((vec![format!("remove {id};")], false))
        }
        "playbgm" | "play_bgm" | "bgm" | "playmp3" => {
            let path = legacy_asset_path(&unquote(&one("BGM requires an asset")?), asset_root);
            Ok((
                vec![format!("play bgm asset({}) loop;", quote_modern(&path))],
                false,
            ))
        }
        "dwave" | "playse" | "play_se" => {
            let path = legacy_asset_path(&unquote(&one("SE requires an asset")?), asset_root);
            Ok((
                vec![format!("play se asset({});", quote_modern(&path))],
                false,
            ))
        }
        "dwaveloop" => {
            let path = legacy_asset_path(&unquote(&one("SE requires an asset")?), asset_root);
            Ok((
                vec![format!("play se asset({}) loop;", quote_modern(&path))],
                false,
            ))
        }
        "stopbgm" | "stop_bgm" | "mp3fadeout" => Ok((vec!["stop bgm;".to_owned()], false)),
        "dwavestop" | "stopse" | "stop_se" => Ok((vec!["stop se;".to_owned()], false)),
        "voice_stop" | "voicestop" => Ok((vec!["stop voice;".to_owned()], false)),
        "save" => {
            let slot = parse_u32(&one("save requires a slot")?)?;
            Ok((vec![format!("save {slot};")], false))
        }
        "load" => {
            let slot = parse_u32(&one("load requires a slot")?)?;
            Ok((vec![format!("load {slot};")], false))
        }
        "include" | "use" => Ok((
            vec!["// legacy include is represented by the generated import graph".to_owned()],
            false,
        )),
        _ => bail!("unsupported legacy command '{name}'"),
    }
}

fn label_target(value: &str, labels: &[String]) -> Result<String> {
    let raw = value.trim().trim_start_matches('*');
    let target = modern_identifier(raw)?;
    if !labels.contains(&target) {
        bail!("target label '{raw}' is missing");
    }
    Ok(target)
}

fn modern_identifier(value: &str) -> Result<String> {
    let mut output = String::new();
    for character in value.trim().chars() {
        if character == '_' || character.is_alphanumeric() {
            output.push(character);
        } else {
            output.push('_');
        }
    }
    if output.is_empty()
        || output
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    {
        bail!("label '{value}' is not a valid Aria 3.1 identifier");
    }
    Ok(output)
}

fn quote_identifier(value: &str) -> Result<String> {
    modern_identifier(value)
}

fn quote_modern(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn legacy_asset_path(path: &str, asset_root: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with('#') || path.starts_with(&format!("{asset_root}/")) {
        path
    } else {
        format!("{asset_root}/{path}")
    }
}

fn sprite_identifier(value: &str) -> Result<String> {
    let value = value.trim().trim_start_matches('$');
    if let Ok(id) = value.parse::<u32>() {
        return Ok(format!("sprite_{id}"));
    }
    modern_identifier(value)
}

fn parse_i32(value: &str) -> Result<i32> {
    value
        .trim()
        .parse::<i32>()
        .with_context(|| format!("expected integer, found '{value}'"))
}

fn parse_u32(value: &str) -> Result<u32> {
    value
        .trim()
        .trim_end_matches("ms")
        .parse::<u32>()
        .with_context(|| format!("expected non-negative integer, found '{value}'"))
}

fn relative_import_path(from: &str, to: &str) -> String {
    let from_parent = from.rsplit_once('/').map_or("", |(parent, _)| parent);
    let from_parts = if from_parent.is_empty() {
        Vec::new()
    } else {
        from_parent.split('/').collect::<Vec<_>>()
    };
    let to_parts = to.split('/').collect::<Vec<_>>();
    let mut shared = 0;
    while shared < from_parts.len()
        && shared < to_parts.len().saturating_sub(1)
        && from_parts[shared] == to_parts[shared]
    {
        shared += 1;
    }
    let mut result = String::new();
    for _ in shared..from_parts.len() {
        result.push_str("../");
    }
    result.push_str(&to_parts[shared..].join("/"));
    result
}

fn modern_source_failures(root: &Path, _manifest_path: &Path) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    for path in project_files(root, |entry| {
        entry.path().extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("aria")
                && entry.file_name() != std::ffi::OsStr::new("init.aria")
        })
    })? {
        let logical = crate::project::logical_path(root, &path)?;
        let source = fs::read_to_string(&path)?;
        let parsed = parse_modern(&logical, source);
        if parsed.has_errors() || parsed.module.is_none() {
            failures.push(logical);
        }
    }
    Ok(failures)
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |relative| relative.to_string_lossy().replace('\\', "/"),
    )
}

fn inferred_game_id(root: &Path) -> String {
    let slug = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("game")
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    format!("local.{}", if slug.is_empty() { "game" } else { &slug })
}

fn backup_inputs(root: &Path, backup: &Path) -> Result<()> {
    fs::create_dir_all(backup)?;
    let files = project_files(root, |entry| is_backup_input(root, entry.path()))?;
    for file in files {
        let relative = file.strip_prefix(root)?;
        let destination = backup.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&file, destination)?;
    }
    Ok(())
}

fn is_backup_input(root: &Path, path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let in_saves = path
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|component| component.as_os_str() == "saves");
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "aria" | "pak" | "ariasav"
    ) || file_name == "config.json"
        || (in_saves && extension.eq_ignore_ascii_case("json"))
        || (file_name.starts_with("save_data_") && extension.eq_ignore_ascii_case("json"))
}

fn migrate_saves(root: &Path, game_id: &str, timestamp: u64) -> Result<StagedSaves> {
    let candidates = project_files(root, |entry| {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        extension.eq_ignore_ascii_case("ariasav")
            || (extension.eq_ignore_ascii_case("json")
                && (path
                    .components()
                    .any(|component| component.as_os_str() == "saves")
                    || name.starts_with("save_data_")
                    || name.starts_with("slot_")))
    })?;
    let mut migrated = 0;
    let mut failed = Vec::new();
    let mut staged = Vec::new();
    for path in candidates {
        let bytes = fs::read(&path)?;
        match decode_legacy_save(&bytes) {
            Ok(payload) => {
                let envelope =
                    SaveEnvelopeV3::new(game_id, aria_core::ENGINE_VERSION, timestamp, &payload)?;
                let slot = slot_from_name(&path).unwrap_or(migrated as u32);
                staged.push((
                    root.join("saves-v3")
                        .join(format!("slot_{slot:04}.ariasave")),
                    envelope.encode()?,
                ));
                migrated += 1;
            }
            Err(error) => failed.push(format!("{}: {error:#}", path.display())),
        }
    }
    Ok((migrated, failed, staged))
}

fn decode_legacy_save(bytes: &[u8]) -> Result<serde_json::Value> {
    if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(b'{')
    {
        return Ok(serde_json::from_slice(bytes)?);
    }
    let (magic, key_material) = if bytes.starts_with(b"ARIASAVE3") {
        (
            b"ARIASAVE3".as_slice(),
            b"AriaEngine.LocalSave.Format.v3".as_slice(),
        )
    } else if bytes.starts_with(b"ARIASAVE2") {
        (
            b"ARIASAVE2".as_slice(),
            b"AriaEngine.LocalSave.Format.v2".as_slice(),
        )
    } else {
        bail!("unknown legacy save header");
    };
    let mut cursor = magic.len();
    let _version = take_i32(bytes, &mut cursor)?;
    let iv_length = usize::try_from(take_i32(bytes, &mut cursor)?)?;
    if iv_length != 16 {
        bail!("invalid AES IV length {iv_length}");
    }
    let iv = take_slice(bytes, &mut cursor, iv_length)?;
    let cipher_length = usize::try_from(take_i32(bytes, &mut cursor)?)?;
    let cipher = take_slice(bytes, &mut cursor, cipher_length)?;
    if cursor != bytes.len() {
        bail!("trailing bytes in legacy save");
    }
    let key: [u8; 32] = Sha256::digest(key_material).into();
    let decrypted = Aes256CbcDecoder::new(&key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(cipher)
        .map_err(|_| anyhow::anyhow!("legacy save AES padding is invalid"))?;
    let mut decoder = GzDecoder::new(decrypted.as_slice());
    let mut json = Vec::new();
    decoder.read_to_end(&mut json)?;
    Ok(serde_json::from_slice(&json)?)
}

fn take_i32(bytes: &[u8], cursor: &mut usize) -> Result<i32> {
    let slice = take_slice(bytes, cursor, 4)?;
    Ok(i32::from_le_bytes(
        slice.try_into().expect("four-byte slice"),
    ))
}

fn take_slice<'a>(bytes: &'a [u8], cursor: &mut usize, length: usize) -> Result<&'a [u8]> {
    let end = cursor
        .checked_add(length)
        .context("legacy save length overflow")?;
    let slice = bytes.get(*cursor..end).context("truncated legacy save")?;
    *cursor = end;
    Ok(slice)
}

fn slot_from_name(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    stem.split(|character: char| !character.is_ascii_digit())
        .rfind(|part| !part.is_empty())?
        .parse()
        .ok()
}

fn rebuild_assets(root: &Path, game_id: &str) -> Result<Option<Vec<u8>>> {
    let asset_root = root.join("assets");
    if !asset_root.is_dir() {
        return Ok(None);
    }
    let mut assets = Vec::new();
    for entry in WalkDir::new(&asset_root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!(
                "symbolic links are not allowed in migrated assets: {}",
                entry.path().display()
            );
        }
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("aria"))
        {
            continue;
        }
        let logical_path = crate::project::logical_path(root, entry.path())?;
        assets.push(AssetInput {
            logical_path,
            bytes: fs::read(entry.path())?,
        });
    }
    let pak = PakArchive::build(game_id, assets)?;
    Ok(Some(pak))
}

fn project_files(root: &Path, predicate: impl Fn(&DirEntry) -> bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(migration_walk_entry)
    {
        let entry = entry?;
        if entry.file_type().is_file() && predicate(&entry) {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}

fn migration_walk_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    !matches!(
        entry.file_name().to_string_lossy().as_ref(),
        ".aria-migrate-backup"
            | ".git"
            | "target"
            | "dist"
            | "bin"
            | "obj"
            | "saves-v3"
            | "v3-data"
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.commit()?;
    Ok(())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_after_backup_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("assets/scripts")).unwrap();
        fs::write(
            root.join("init.aria"),
            "window 1280, 720, \"Legacy\"\nscript \"assets/scripts/main.aria\"\n",
        )
        .unwrap();
        fs::write(
            root.join("assets/scripts/main.aria"),
            "# aria-version: 2.0\nミオ「移行する。」\nend\n",
        )
        .unwrap();
        fs::write(root.join("config.json"), "{\"BgmVolume\": 50}").unwrap();
        fs::create_dir_all(root.join("saves")).unwrap();
        fs::write(
            root.join("saves/slot_01.json"),
            "{\"Runtime\": {\"pc\": 3}}",
        )
        .unwrap();

        let report = migrate_project(root, Some("jp.example.legacy"), 42).unwrap();
        assert!(!report.already_v3);
        assert_eq!(report.saves_migrated, 1);
        assert!(root.join("aria.toml").is_file());
        let manifest =
            ProjectManifest::from_toml(&fs::read_to_string(root.join("aria.toml")).unwrap())
                .unwrap();
        assert_eq!(manifest.game.version, "1.0.0");
        assert!(
            root.join(".aria-migrate-backup/42/assets/scripts/main.aria")
                .is_file()
        );
        let original =
            fs::read_to_string(root.join(".aria-migrate-backup/42/assets/scripts/main.aria"))
                .unwrap();
        assert!(original.contains("2.0"));
        let migrated = fs::read_to_string(root.join("assets/scripts/main.aria")).unwrap();
        assert!(migrated.starts_with("aria 3.1;\nentry start;\n"));
        assert!(migrated.contains("await advance;"));

        let second = migrate_project(root, None, 43).unwrap();
        assert!(second.already_v3);
        assert_eq!(second.compiler_errors_after_migration, 0);
        assert!(!root.join(".aria-migrate-backup/43").exists());
    }

    #[test]
    fn existing_v3_manifest_is_revalidated_instead_of_assumed_successful() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("assets/scripts")).unwrap();
        fs::write(
            root.join("aria.toml"),
            "schema = 3\n\
             [game]\n\
             id = \"jp.example.invalid\"\n\
             version = \"3.0.0\"\n\
             title = \"invalid\"\n\
             [runtime]\n\
             entry = \"assets/scripts/main.aria\"\n\
             logical_width = 1280\n\
             logical_height = 720\n\
             asset_roots = [\"assets\"]\n\
             fonts = []\n\
             save_namespace = \"invalid\"\n",
        )
        .unwrap();
        fs::write(
            root.join("assets/scripts/main.aria"),
            "# aria-version: 3.0\nstrict on\ngoto *missing\n",
        )
        .unwrap();

        let report = migrate_project(root, None, 44).unwrap();
        assert!(report.already_v3);
        assert!(report.compiler_errors_after_migration > 0);
        assert!(
            report
                .notices
                .iter()
                .any(|notice| notice.contains("revalidated"))
        );
        assert!(root.join("aria-migrate-report.json").is_file());
        assert!(!root.join(".aria-migrate-backup/44").exists());
    }

    #[test]
    fn migration_preflights_config_before_mutating_legacy_sources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("assets/scripts")).unwrap();
        fs::write(
            root.join("init.aria"),
            "script \"assets/scripts/main.aria\"\n",
        )
        .unwrap();
        let source = "# aria-version: 2.0\nミオ「そのまま。」\nend\n";
        fs::write(root.join("assets/scripts/main.aria"), source).unwrap();
        fs::write(root.join("config.json"), "{not-json").unwrap();

        assert!(migrate_project(root, None, 45).is_err());
        assert_eq!(
            fs::read_to_string(root.join("assets/scripts/main.aria")).unwrap(),
            source
        );
        assert!(!root.join("aria.toml").exists());
        assert!(
            root.join(".aria-migrate-backup/45/assets/scripts/main.aria")
                .is_file()
        );
    }

    #[test]
    fn migration_does_not_create_an_asset_root_before_a_preflight_failure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("init.aria"), "script \"main.aria\"\n").unwrap();
        fs::write(
            root.join("main.aria"),
            "# aria-version: 2.0\nミオ「移行する。」\nend\n",
        )
        .unwrap();
        fs::write(root.join("config.json"), "not-json").unwrap();

        assert!(migrate_project(root, None, 46).is_err());
        assert!(!root.join("assets").exists());
        assert_eq!(
            fs::read_to_string(root.join("main.aria")).unwrap(),
            "# aria-version: 2.0\nミオ「移行する。」\nend\n"
        );
    }

    #[test]
    fn migration_reports_missing_converted_assets_without_writing_sources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("init.aria"), "script \"main.aria\"\n").unwrap();
        let source = "# aria-version: 2.0\nbg \"missing.png\"\nend\n";
        fs::write(root.join("main.aria"), source).unwrap();

        let report = migrate_project(root, None, 47).unwrap();
        assert!(!report.scripts_failed.is_empty());
        assert!(!root.join("aria.toml").exists());
        assert_eq!(fs::read_to_string(root.join("main.aria")).unwrap(), source);
    }
}
