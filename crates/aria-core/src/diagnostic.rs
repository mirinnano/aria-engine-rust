use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable diagnostic identifiers used by the V3 compiler and migration tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DiagnosticCode {
    InvalidManifest,
    InvalidSyntax,
    UnsupportedLanguageVersion,
    DuplicateLabel,
    UnknownLabel,
    InvalidOperand,
    MissingSource,
    InvalidControlFlow,
    UnsupportedRuntimeCommand,
    MigrationNotice,
    UnknownCommand,
    AmbiguousBareText,
}

impl DiagnosticCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidManifest => "E001",
            Self::InvalidSyntax => "E100",
            Self::UnsupportedLanguageVersion => "E101",
            Self::DuplicateLabel => "E102",
            Self::UnknownLabel => "E103",
            Self::InvalidOperand => "E104",
            Self::MissingSource => "E105",
            Self::InvalidControlFlow => "E106",
            Self::UnsupportedRuntimeCommand => "W300",
            Self::MigrationNotice => "W301",
            Self::UnknownCommand => "E107",
            Self::AmbiguousBareText => "W302",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub length: u32,
}

impl SourceSpan {
    #[must_use]
    pub fn line(source: impl Into<String>, line: u32, length: usize) -> Self {
        Self {
            source: source.into(),
            line,
            column: 1,
            length: u32::try_from(length).unwrap_or(u32::MAX),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: Option<SourceSpan>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(
        code: DiagnosticCode,
        message: impl Into<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            span,
        }
    }

    #[must_use]
    pub fn warning(
        code: DiagnosticCode,
        message: impl Into<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        if let Some(span) = &self.span {
            write!(
                formatter,
                "{}:{}:{}: {severity}[{}]: {}",
                span.source,
                span.line,
                span.column,
                self.code.as_str(),
                self.message
            )
        } else {
            write!(
                formatter,
                "{severity}[{}]: {}",
                self.code.as_str(),
                self.message
            )
        }
    }
}
