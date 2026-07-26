//! Project-source boundary for the single Aria language.
//!
//! The compiler accepts only the structured, ownership-aware `aria;` syntax.
//! It intentionally contains no mode dispatch, compatibility parser, or host
//! command bridge. The modern front end owns parsing, semantic analysis, and
//! lowering; this module owns only deterministic source collection and path
//! validation shared by CLI and embedding hosts.

use std::collections::BTreeMap;

use crate::bytecode::CompiledProgram;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceUnit {
    pub logical_path: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileInput {
    pub game_id: String,
    pub entry: String,
    pub sources: Vec<SourceUnit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompileOutput {
    pub program: Option<CompiledProgram>,
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileOutput {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}

/// Compiles a complete import closure using Aria's single source language.
#[must_use]
pub fn compile(input: CompileInput) -> CompileOutput {
    let mut diagnostics = Vec::new();
    let mut sources = BTreeMap::new();
    let mut portable_source_names = BTreeMap::new();
    for source in input.sources {
        match normalize_logical_path(&source.logical_path) {
            Ok(path) => {
                let portable_name =
                    portable_path_key(&path).expect("a normalized logical path has a portable key");
                if let Some(existing) = portable_source_names.insert(portable_name, path.clone())
                    && existing != path
                {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidSyntax,
                        format!(
                            "source paths '{existing}' and '{path}' collide on a case-insensitive filesystem"
                        ),
                        None,
                    ));
                }
                if sources.insert(path.clone(), source.source).is_some() {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidSyntax,
                        format!("duplicate source '{path}'"),
                        None,
                    ));
                }
            }
            Err(message) => diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidSyntax,
                message,
                None,
            )),
        }
    }

    let entry = match normalize_logical_path(&input.entry) {
        Ok(entry) => entry,
        Err(message) => {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::MissingSource,
                message,
                None,
            ));
            return CompileOutput {
                program: None,
                diagnostics,
            };
        }
    };
    if !sources.contains_key(&entry) {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::MissingSource,
            format!("entry source '{entry}' is missing"),
            None,
        ));
        return CompileOutput {
            program: None,
            diagnostics,
        };
    }

    crate::modern_compiler::compile_modern(input.game_id, entry, sources, diagnostics)
}

/// Normalizes one logical project path and rejects host-specific spellings.
pub fn normalize_logical_path(path: &str) -> Result<String, String> {
    let path = path.replace('\\', "/");
    if path.starts_with('/') || path.as_bytes().get(1).is_some_and(|byte| *byte == b':') {
        return Err(format!("logical path must be relative: '{path}'"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("logical path escapes project root: '{path}'"));
                }
            }
            other => {
                if other.as_bytes().contains(&0) {
                    return Err(format!("logical path contains a NUL byte: '{path}'"));
                }
                parts.push(other.nfc().collect::<String>());
            }
        }
    }
    if parts.is_empty() {
        return Err("logical path is empty".to_owned());
    }
    Ok(parts.join("/"))
}

/// Comparison key used to reject paths that collide on a case-insensitive
/// filesystem. Packaged paths themselves remain case-sensitive.
pub fn portable_path_key(path: &str) -> Result<String, String> {
    let normalized = normalize_logical_path(path)?;
    Ok(normalized
        .chars()
        .flat_map(char::to_lowercase)
        .collect::<String>())
}

pub(crate) fn resolve_logical_path(source: &str, requested: &str) -> Result<String, String> {
    let parent = source.rsplit_once('/').map_or("", |(parent, _)| parent);
    let combined = if parent.is_empty() {
        requested.to_owned()
    } else {
        format!("{parent}/{requested}")
    };
    normalize_logical_path(&combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_script(source: &str) -> CompileOutput {
        compile(CompileInput {
            game_id: "jp.example.test".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![SourceUnit {
                logical_path: "scripts/main.aria".to_owned(),
                source: source.to_owned(),
            }],
        })
    }

    #[test]
    fn rejects_line_oriented_compatibility_syntax() {
        let output = compile_script("# aria-version: 3.0\n*start\nミオ「海へ行こう。」\nend\n");
        assert!(output.has_errors());
        assert!(output.program.is_none());
    }

    #[test]
    fn logical_paths_are_nfc_and_reject_case_insensitive_source_collisions() {
        assert_eq!(
            normalize_logical_path("assets/re\u{301}sume\u{301}.png").unwrap(),
            "assets/résumé.png"
        );
        assert_eq!(
            portable_path_key("assets/Mio.PNG").unwrap(),
            portable_path_key("assets/mio.png").unwrap()
        );

        let output = compile(CompileInput {
            game_id: "jp.example.test".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![
                SourceUnit {
                    logical_path: "scripts/main.aria".to_owned(),
                    source: "aria;\nentry start;\nscene start { end; }\n".to_owned(),
                },
                SourceUnit {
                    logical_path: "scripts/Main.aria".to_owned(),
                    source: "aria;\nentry start;\nscene start { end; }\n".to_owned(),
                },
            ],
        });
        assert!(output.has_errors());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("case-insensitive"))
        );
    }
}
