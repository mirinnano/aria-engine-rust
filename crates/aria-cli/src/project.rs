use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aria_core::bytecode::{ByteOp, Constant, Operand};
use aria_core::{
    CompileInput, CompileOutput, CompiledProgram, Diagnostic, DiagnosticCode, ProjectManifest,
    SourceSpan, SourceUnit, compile,
};
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone)]
pub struct LoadedProject {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
}

/// The exact, canonical logical paths that make up a loose V3 project.
///
/// This inventory is deliberately built once and shared by `check`, `build`,
/// and the loose Native Player. A file that only resolves because Windows is
/// case-insensitive, or because a host normalizes Unicode differently, is not
/// a V3 asset path.
#[derive(Debug, Clone)]
pub struct AssetInventory {
    entries: BTreeMap<String, PathBuf>,
}

impl AssetInventory {
    #[must_use]
    #[cfg(test)]
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
                "release requires at least one runtime.fonts asset; V3 Players never use system fonts"
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
        let mut output = compile(CompileInput {
            game_id: self.manifest.game.id.clone(),
            entry: self.manifest.runtime.entry.clone(),
            sources: self.sources()?,
        });
        if let Some(program) = &output.program {
            output
                .diagnostics
                .extend(validate_program_asset_references(program, &assets));
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
            ".git" | ".aria-migrate-backup" | "target" | "dist" | "bin" | "obj" | "node_modules"
        ))
}

fn validate_program_asset_references(
    program: &CompiledProgram,
    assets: &AssetInventory,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut reported = BTreeSet::new();
    for (instruction_index, instruction) in program.instructions.iter().enumerate() {
        let asset_operand = match instruction.op {
            ByteOp::Background => Some((0, true, "background")),
            ByteOp::SpriteImage => Some((1, false, "image")),
            ByteOp::PlayAudio => Some((2, false, "audio")),
            _ => None,
        };
        let Some((operand_index, accepts_color, kind)) = asset_operand else {
            continue;
        };
        let Some(operand) = instruction.operands.get(operand_index) else {
            continue;
        };
        let Some(path) = constant_string(program, operand) else {
            // The structured 3.1 front end only emits literal asset paths.
            // Keep the alpha bridge diagnosable without pretending a dynamic
            // legacy register has an exact package identity.
            if program.language_version == aria_core::LanguageVersion::V3_1 {
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
