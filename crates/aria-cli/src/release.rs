//! Release-only validation that deliberately keeps incomplete migration work
//! from being packaged as a playable game.

use aria_core::bytecode::{ByteOp, LanguageVersion};
use aria_core::{CompileOutput, DiagnosticCode};

/// Counts source commands that would become `RuntimeCommand::Unsupported`.
///
/// Count both diagnostics and bytecode so this remains a release guard even if
/// a compiler regression accidentally omits its W300 diagnostic.
#[must_use]
pub fn unsupported_runtime_command_count(output: &CompileOutput) -> usize {
    let diagnostic_count = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedRuntimeCommand)
        .count();
    let bytecode_count = output.program.as_ref().map_or(0, |program| {
        program
            .instructions
            .iter()
            .filter(|instruction| instruction.op == ByteOp::Host)
            .count()
    });
    diagnostic_count.max(bytecode_count)
}

/// The V3.0 line-command bridge exists only to make source migration
/// inspectable. A 1.0-quality package must be authored and compiled as the
/// structured Aria 3.1 language; otherwise a release could silently retain
/// legacy coercions or host-command semantics.
#[must_use]
pub fn has_release_language(output: &CompileOutput) -> bool {
    output
        .program
        .as_ref()
        .is_some_and(|program| program.language_version == LanguageVersion::V3_1)
}
