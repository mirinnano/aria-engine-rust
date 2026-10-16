use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aria_core::bytecode::{ByteOp, Constant, Operand};
use aria_core::{
    CompileInput, CompileOutput, CompiledProgram, Diagnostic, DiagnosticCode, ProjectManifest,
    SourceSpan, SourceUnit,
};
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone)]
pub struct LoadedProject {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
}

/// The exact, canonical logical paths that make up a loose Aria project.
///
/// This inventory is deliberately built once and shared by `check`, `build`,
/// and the loose Native Player. A file that only resolves because Windows is
/// case-insensitive, or because a host normalizes Unicode differently, is not
/// an Aria asset path.
#[derive(Debug, Clone)]
pub struct AssetInventory {
    entries: BTreeMap<String, PathBuf>,
}

impl AssetInventory {
    #[must_use]
    #[cfg(all(test, feature = "desktop-player", not(target_arch = "wasm32")))]
    pub(crate) fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn contains(&self, logical_path: &str) -> bool {
        self.entries.contains_key(logical_path)
    }

    #[must_use]
    pub fn path(&self, logical_path: &str) -> Option<&Path> {
        self.entries.get(logical_path).map(PathBuf::as_path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.entries
            .iter()
            .map(|(logical_path, path)| (logical_path.as_str(), path.as_path()))
    }
}

impl LoadedProject {
    pub fn load(path: &Path) -> Result<Self> {
        let manifest_path = if path.is_dir() {
            path.join("aria.toml")
        } else if path.file_name().is_some_and(|name| name == "aria.toml") {
            path.to_owned()
        } else {
            bail!(
                "project must be a directory or aria.toml: {}",
                path.display()
            );
        };
        let root = manifest_path
            .parent()
            .context("aria.toml has no parent directory")?
            .canonicalize()
            .with_context(|| format!("cannot resolve project root {}", path.display()))?;
        let source = fs::read_to_string(&manifest_path)
            .with_context(|| format!("cannot read {}", manifest_path.display()))?;
        let manifest = ProjectManifest::from_toml(&source)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(Self { root, manifest })
    }

    /// Creates a validated build-only view of this project.  A content
    /// variant may select a smaller import closure and a separate save
    /// namespace without mutating the author's `aria.toml` on disk.
    ///
    /// This is intentionally an explicit copy rather than an environment
    /// lookup: the selected entry and save namespace become ordinary bundle
    /// metadata, which keeps demo and full-release builds reproducible.
    pub fn with_runtime_overrides(
        &self,
        entry: Option<&str>,
        save_namespace: Option<&str>,
    ) -> Result<Self> {
        let mut manifest = self.manifest.clone();
        if let Some(entry) = entry {
            manifest.runtime.entry = entry.to_owned();
        }
        if let Some(save_namespace) = save_namespace {
            manifest.runtime.save_namespace = save_namespace.to_owned();
            // A content-limited edition must never silently remove the
            // full game's records just because it shares source assets.
            manifest.runtime.legacy_save_namespaces.clear();
        }
        manifest
            .validate()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(Self {
            root: self.root.clone(),
            manifest,
        })
    }

    /// The project's own plugin folder, `<project>/plugins/`.
    #[must_use]
    pub fn plugin_directory(&self) -> PathBuf {
        self.root.join(crate::plugins::PLUGIN_DIRECTORY)
    }

    /// Verifies and selects the project's plugins, and rejects cross-plugin
    /// route collisions before a check, build, or run executes any prose.
    pub fn plugin_selection(&self) -> Result<aria_plugin_host::select::Selection> {
        let selection =
            crate::plugins::discover(&[self.plugin_directory()], &self.manifest.plugins)?;
        let mut registry = aria_core::PluginRegistry::default();
        for package in &selection.plugins {
            registry.register(package.registration.clone())?;
        }
        Ok(selection)
    }

    pub fn plugins(&self) -> Result<Vec<aria_core::PluginRegistration>> {
        Ok(crate::plugins::registrations(&self.plugin_selection()?))
    }

    pub fn sources(&self) -> Result<Vec<SourceUnit>> {
        let mut sources = Vec::new();
        for entry in WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(include_project_entry)
        {
            let entry = entry?;
            if !entry.file_type().is_file()
                || !entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("aria"))
            {
                continue;
            }
            let logical_path = logical_path(&self.root, entry.path())?;
            let source = fs::read_to_string(entry.path())
                .with_context(|| format!("cannot read source {}", entry.path().display()))?;
            sources.push(SourceUnit {
                logical_path,
                source,
            });
        }
        sources.sort_by(|left, right| left.logical_path.cmp(&right.logical_path));
        Ok(sources)
    }

    /// Returns the canonical project asset inventory without reading payloads.
    pub fn asset_inventory(&self) -> Result<AssetInventory> {
        let mut entries = BTreeMap::new();
        let mut portable_names = BTreeMap::new();
        let excluded = self
            .manifest
            .runtime
            .asset_excludes
            .iter()
            .collect::<BTreeSet<_>>();
        for root in &self.manifest.runtime.asset_roots {
            let disk_root = self.root.join(root);
            let metadata = fs::symlink_metadata(&disk_root)
                .with_context(|| format!("asset root is missing: {}", disk_root.display()))?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "asset root must not be a symbolic link: {}",
                    disk_root.display()
                );
            }
            if !metadata.is_dir() {
                bail!("asset root is not a directory: {}", disk_root.display());
            }
            for entry in WalkDir::new(&disk_root).follow_links(false) {
                let entry = entry?;
                if entry.file_type().is_symlink() {
                    bail!(
                        "symbolic links are not allowed in runtime.asset_roots: {}",
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
                let logical_path = logical_path(&self.root, entry.path())?;
                if excluded.contains(&logical_path) {
                    continue;
                }
                let portable_name = aria_core::compiler::portable_path_key(&logical_path)
                    .map_err(|error| anyhow::anyhow!(error))?;
                if let Some(existing) = portable_names.insert(portable_name, logical_path.clone())
                    && existing != logical_path
                {
                    bail!(
                        "asset paths '{existing}' and '{logical_path}' collide on a case-insensitive filesystem"
                    );
                }
                if entries
                    .insert(logical_path.clone(), entry.path().to_owned())
                    .is_some()
                {
                    bail!("asset appears more than once: {logical_path}");
                }
            }
        }
        Ok(AssetInventory { entries })
    }

    /// Validates the release-only font contract before data is packaged.
    ///
    /// Development smoke projects may intentionally have an empty list, but
    /// a release cannot rely on a host font. `fontdb` parses each selected
    /// byte stream here so malformed files fail before a Player ships.
    pub fn validate_bundled_fonts(
        &self,
        assets: &AssetInventory,
        require_at_least_one: bool,
    ) -> Result<()> {
        if require_at_least_one && self.manifest.runtime.fonts.is_empty() {
            bail!(
                "release requires at least one runtime.fonts asset; Aria Players never use system fonts"
            );
        }
        let mut database = fontdb::Database::new();
        for logical_path in &self.manifest.runtime.fonts {
            let disk_path = assets.path(logical_path).ok_or_else(|| {
                anyhow::anyhow!(
                    "runtime.fonts asset '{logical_path}' is missing from runtime.asset_roots"
                )
            })?;
            let before = database.len();
            database
                .load_font_data(fs::read(disk_path).with_context(|| {
                    format!("cannot read bundled font {}", disk_path.display())
                })?);
            if database.len() == before {
                bail!(
                    "runtime.fonts asset '{logical_path}' is not a readable OpenType/TrueType font"
                );
            }
        }
        Ok(())
    }

    pub fn compile(&self) -> Result<CompileOutput> {
        let assets = self.asset_inventory()?;
        self.validate_bundled_fonts(&assets, false)?;
        let manifests = self
            .plugins()?
            .into_iter()
            .map(|plugin| plugin.manifest)
            .collect::<Vec<_>>();
        let mut output = aria_core::compiler::compile_with_plugins(
            CompileInput {
                game_id: self.manifest.game.id.clone(),
                entry: self.manifest.runtime.entry.clone(),
                sources: self.sources()?,
            },
            &manifests,
        );
        if let Some(program) = &output.program {
            output
                .diagnostics
                .extend(validate_program_asset_references(program, &assets));
            output.diagnostics.extend(validate_stage_cues(
                program,
                &self.manifest.presentation.stages,
            ));
        }
        Ok(output)
    }
}

pub fn logical_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("{} is outside {}", path.display(), root.display()))?;
    let parts = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_owned)
                .context("project paths must be valid UTF-8")
        })
        .collect::<Result<Vec<_>>>()?;
    let raw = parts.join("/");
    let normalized = aria_core::compiler::normalize_logical_path(&raw)
        .map_err(|error| anyhow::anyhow!(error))?;
    if normalized != raw {
        bail!(
            "project path '{}' must use canonical NFC '/' spelling; use '{normalized}'",
            path.display()
        );
    }
    Ok(normalized)
}

fn include_project_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !(entry.file_type().is_dir()
        && matches!(
            name.as_ref(),
            ".git" | "target" | "dist" | "bin" | "obj" | "node_modules"
        ))
}

fn validate_program_asset_references(
    program: &CompiledProgram,
    assets: &AssetInventory,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut reported = BTreeSet::new();
    for (instruction_index, instruction) in program.instructions.iter().enumerate() {
        let asset_operand = asset_operand(instruction.op);
        let Some((operand_index, accepts_color, kind)) = asset_operand else {
            continue;
        };
        let Some(operand) = instruction.operands.get(operand_index) else {
            continue;
        };
        let Some(path) = constant_string(program, operand) else {
            // The single Aria source language emits literal asset paths. A
            // non-literal path is never packageable because it has no stable
            // asset identity.
            if program.language_version == aria_core::LanguageVersion::CURRENT {
                diagnostics.push(asset_diagnostic(
                    program,
                    instruction_index,
                    format!("{kind} asset paths must be static canonical logical asset references"),
                ));
            }
            continue;
        };
        if accepts_color && path.starts_with('#') {
            continue;
        }
        let canonical = aria_core::compiler::normalize_logical_path(path);
        let valid = canonical
            .as_ref()
            .is_ok_and(|normalized| normalized == path && assets.contains(normalized));
        if valid {
            continue;
        }
        let message = match canonical {
            Ok(normalized) if normalized != path => format!(
                "{kind} asset '{path}' is not a canonical NFC '/' logical path; use '{normalized}'"
            ),
            Ok(_) => format!(
                "{kind} asset '{path}' is missing from runtime.asset_roots or differs in case/Unicode spelling"
            ),
            Err(error) => format!("{kind} asset '{path}' is invalid: {error}"),
        };
        let key = (instruction_index, path.to_owned());
        if reported.insert(key) {
            diagnostics.push(asset_diagnostic(program, instruction_index, message));
        }
    }
    diagnostics
}

/// When `[presentation] stages` lists the cue names the frontend knows, a
/// `stage` outside it is a typo the presentation would silently ignore.
fn validate_stage_cues(program: &CompiledProgram, known: &[String]) -> Vec<Diagnostic> {
    if known.is_empty() {
        return Vec::new();
    }
    program
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, instruction)| instruction.op == ByteOp::SetStage)
        .filter_map(|(index, instruction)| {
            let cue = constant_string(program, instruction.operands.first()?)?;
            (!known.iter().any(|name| name == cue)).then(|| {
                asset_diagnostic(
                    program,
                    index,
                    format!(
                        "stage '{cue}' is not in [presentation] stages; known: {}",
                        known.join(", ")
                    ),
                )
            })
        })
        .collect()
}

/// Returns the literal assets reachable from the selected source import
/// closure. Manifest fonts are deliberately not included here: every Player
/// needs those regardless of whether the story mentions them explicitly.
pub(crate) fn referenced_asset_paths(program: &CompiledProgram) -> BTreeSet<String> {
    program
        .instructions
        .iter()
        .filter_map(|instruction| {
            let (operand_index, accepts_color, _) = asset_operand(instruction.op)?;
            let path = constant_string(program, instruction.operands.get(operand_index)?)?;
            if accepts_color && path.starts_with('#') {
                None
            } else {
                Some(path.to_owned())
            }
        })
        .collect()
}

fn asset_operand(op: ByteOp) -> Option<(usize, bool, &'static str)> {
    match op {
        ByteOp::Background => Some((0, true, "background")),
        ByteOp::SpriteImage => Some((1, false, "image")),
        ByteOp::PlayAudio => Some((2, false, "audio")),
        ByteOp::PreloadAsset => Some((0, false, "preload")),
        ByteOp::DeclareChapter => Some((6, false, "chapter image")),
        _ => None,
    }
}

fn constant_string<'a>(program: &'a CompiledProgram, operand: &Operand) -> Option<&'a str> {
    let Operand::Constant(index) = operand else {
        return None;
    };
    let constant = program.constants.get(usize::try_from(*index).ok()?)?;
    let Constant::String(value) = constant else {
        return None;
    };
    Some(value)
}

fn asset_diagnostic(
    program: &CompiledProgram,
    instruction_index: usize,
    message: String,
) -> Diagnostic {
    let span = program
        .source_map
        .get(instruction_index)
        .map(|location| SourceSpan {
            source: location.source.clone(),
            line: location.line,
            column: location.column,
            length: 1,
        });
    Diagnostic::error(DiagnosticCode::MissingSource, message, span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{CompileInput, SourceUnit, compile};

    fn program(source: &str) -> CompiledProgram {
        compile(CompileInput {
            game_id: "jp.example.stages".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![SourceUnit {
                logical_path: "scripts/main.aria".to_owned(),
                source: source.to_owned(),
            }],
        })
        .program
        .unwrap()
    }

    #[test]
    fn stage_cues_outside_the_declared_vocabulary_are_errors() {
        let program = program(
            "aria;\nentry start;\nscene start {\n  stage ward;\n  stage wrad;\n  end;\n}\n",
        );
        let known = vec!["ward".to_owned(), "tide".to_owned()];
        let diagnostics = validate_stage_cues(&program, &known);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("'wrad'"));
        assert_eq!(diagnostics[0].span.as_ref().map(|span| span.line), Some(5));
        // Without a declared vocabulary there is nothing to compare against.
        assert!(validate_stage_cues(&program, &[]).is_empty());
    }
}
