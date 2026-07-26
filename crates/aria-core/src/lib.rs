#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

//! Aria's deterministic runtime core.
//!
//! This crate deliberately owns no window, GPU, audio device, clock, browser,
//! or filesystem handle. Callers provide source bytes, timestamps, input
//! snapshots, and persistence bytes. The core returns stable render, audio,
//! UI, and runtime command streams.

pub mod bytecode;
pub mod compiler;
pub mod diagnostic;
pub mod input;
pub mod modern;
mod modern_compiler;
pub mod pak;
pub mod presentation;
mod presentation_state;
pub mod project;
pub mod protocol;
pub mod save;
pub mod text;
pub mod vm;

pub use bytecode::{AriacError, CompiledProgram, LanguageVersion};
pub use compiler::{CompileInput, CompileOutput, SourceUnit, compile};
pub use diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
pub use input::{InputAction, InputSnapshot, PointerSnapshot};
pub use presentation::{
    ActionView, BacklogEntryView, ChapterView, ChoiceView, ConfirmationView, DialogueView,
    GalleryItemView, GameView, InterludeView, UI_VIEW_MODEL_SCHEMA, UiIntent, UiRoute, UiViewModel,
};
pub use presentation_state::{PendingConfirmation, UiInsets, UiRuntimeState, UiViewport};
pub use project::{PresentationManifest, ProjectManifest, ProjectValidationError};
pub use protocol::{
    AudioCommand, BorderStyle, Color, DrawCommand, DrawStyle, GradientStyle, LogicalSize, Rect,
    RuntimeCommand, SceneFrame, ScreenEffect, ShadowStyle, SpriteFit, StepOutput, TextAlign,
    TextDecoration,
};
pub use save::{SaveEnvelopeError, SaveEnvelopeV3};
pub use vm::{
    AutoMode, BacklogEntryState, ChapterState, SettingsState, SkipMode, VM_SNAPSHOT_SCHEMA, Vm,
    VmError, VmSnapshot,
};

/// Current file formats use this engine version in newly-created envelopes.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
