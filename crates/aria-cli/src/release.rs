//! Release-only validation for the single Aria source language.

use aria_core::CompileOutput;
use aria_core::bytecode::LanguageVersion;

/// A release package must be compiled by the single Aria front end. Source
/// source modes do not exist, and only the current compiled ABI is accepted.
#[must_use]
pub fn has_release_language(output: &CompileOutput) -> bool {
    output
        .program
        .as_ref()
        .is_some_and(|program| program.language_version == LanguageVersion::CURRENT)
}
