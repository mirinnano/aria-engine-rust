#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

//! Aria V3's deterministic runtime core.
//!
//! This crate deliberately owns no window, GPU, audio device, clock, browser,
//! or filesystem handle. Callers provide source bytes, timestamps, input
//! snapshots, and persistence bytes. The core returns stable render, audio,
//! UI, and runtime command streams.

pub mod bytecode;
pub mod compiler;
pub mod diagnostic;
pub mod input;
pub mod migration;
pub mod modern;
mod modern_compiler;
pub mod pak;
pub mod project;
pub mod protocol;
pub mod save;
pub mod syntax;
pub mod text;
pub mod vm;

pub use bytecode::{AriacError, CompiledProgram, LanguageVersion};
pub use compiler::{CompileInput, CompileOutput, SourceUnit, compile};
pub use diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
pub use input::{InputAction, InputSnapshot, PointerSnapshot};
pub use project::{ProjectManifest, ProjectValidationError};
pub use protocol::{AudioCommand, RenderFrame, RuntimeCommand, StepOutput, UiTree};
pub use save::{SaveEnvelopeError, SaveEnvelopeV3};
pub use vm::{Vm, VmError, VmSnapshot};

/// V3 file formats use this engine version in newly-created envelopes.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
