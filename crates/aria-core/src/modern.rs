//! Lossless syntax and AST support for the single, ownership-aware Aria author
//! language.
//!
//! This module deliberately has no dependency on a line-oriented compiler.
//! It is a front-end boundary: callers can retain the concrete source for
//! formatting while consuming the typed, structured AST for semantic analysis
//! and lowering.

use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
/// A lossless parse result for one modern Aria source file.
#[derive(Debug, Clone, PartialEq)]
pub struct ModernParse {
    /// The raw source and all concrete tokens, including trivia.
    pub cst: ModernCst,
    /// The structured module when the required `aria` header could be read.
    pub module: Option<ModernModule>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ModernParse {
    /// Returns true when lexing or parsing produced an error diagnostic.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}

/// Concrete, lossless source data retained for formatters and source tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModernCst {
    pub source_id: String,
    pub source: String,
    pub tokens: Vec<ModernToken>,
}

impl ModernCst {
    /// Returns the input byte-for-byte, including comments and line endings.
    #[must_use]
    pub fn lossless_source(&self) -> &str {
        &self.source
    }
}

/// One token in the concrete syntax tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModernToken {
    pub kind: ModernTokenKind,
    /// Exact source bytes for this token (valid UTF-8 because the source is a
    /// Rust string).  String token text includes its quotes and escapes.
    pub text: String,
    pub span: SourceSpan,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Token kinds used by the modern language lexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModernTokenKind {
    Identifier,
    String,
    Number,
    Whitespace,
    LineComment,
    BlockComment,
    Semicolon,
    Colon,
    Comma,
    Dot,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Equals,
    Plus,
    PlusEquals,
    Minus,
    FatArrow,
    EqualEqual,
    Bang,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    Ampersand,
    Invalid,
    Eof,
}

impl ModernTokenKind {
    fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }
}

/// A parsed modern source module.
#[derive(Debug, Clone, PartialEq)]
pub struct ModernModule {
    pub span: SourceSpan,
    pub name: Option<QualifiedName>,
    pub imports: Vec<ImportDecl>,
    pub entry: Option<EntryDecl>,
    pub states: Vec<StateDecl>,
    pub scenes: Vec<SceneDecl>,
    /// Retired visual declarations retained only long enough to produce a
    /// source-located retirement diagnostic. They are never lowered into a
    /// program, a render protocol, or a VM layout tree.
    pub retired_ui_syntax: Vec<RetiredUiDeclaration>,
}

/// A parsed occurrence of retired visual DSL syntax. The parser keeps only its
/// span so the compiler can explain the break at the exact source location;
/// it has no lowering path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredUiDeclaration {
    pub kind: RetiredUiKind,
    pub span: SourceSpan,
}

/// The former top-level visual DSL declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetiredUiKind {
    Theme,
    Screen,
    Transition,
}

/// A dot-qualified module name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedName {
    pub span: SourceSpan,
    pub segments: Vec<String>,
}

impl QualifiedName {
    #[must_use]
    pub fn as_string(&self) -> String {
        self.segments.join(".")
    }
}

/// A compile-time-only import declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub span: SourceSpan,
    pub path: String,
}

/// The scene selected when a program starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryDecl {
    pub span: SourceSpan,
    pub scene: String,
}

/// A top-level saved state declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct StateDecl {
    pub span: SourceSpan,
    pub mutable: bool,
    pub name: String,
    pub ty: ModernType,
    pub value: Literal,
}

/// The value types in the first modern language subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModernType {
    Int,
    Bool,
    String,
    Node,
}

/// A named, structured visual-novel scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDecl {
    pub span: SourceSpan,
    pub name: String,
    pub body: Vec<Statement>,
}

/// A statement and its source location.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub span: SourceSpan,
    pub kind: StatementKind,
}

/// Structured statements in the modern 3.x subset.
#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Say {
        speaker: Option<String>,
        text: String,
    },
    Narrate {
        text: String,
    },
    ClearDialogue,
    AwaitAdvance,
    Wait {
        duration_ms: u32,
        /// A breath is a soft, player-releasable pause after a minimum floor.
        /// Ordinary `wait` remains an authored hard hold.
        release_after_ms: Option<u32>,
    },
    Background {
        asset: AssetRef,
        transition: Option<Transition>,
    },
    /// Creates a scene node owned by the lexical binding. `drop`, a scene
    /// transfer, or the binding's scope exit deterministically removes it.
    Spawn {
        mutable: bool,
        name: String,
        content: ShowContent,
        z: i32,
    },
    Hide {
        node: NodeAccess,
    },
    Reveal {
        node: NodeAccess,
    },
    /// Consumes the owning node binding and releases the scene resource.
    Drop {
        name: String,
    },
    Move {
        node: NodeAccess,
        position: Position,
    },
    Declare {
        mutable: bool,
        name: String,
        ty: Option<ModernType>,
        value: Value,
    },
    Assign {
        name: String,
        value: Value,
    },
    AddAssign {
        name: String,
        value: i64,
    },
    If {
        condition: Expression,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
    },
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    /// Creates a lexical borrow alias. A mutable borrow exclusively loans the
    /// owner until the block ends; an immutable borrow cannot be used by scene
    /// mutation commands.
    Borrow {
        mutable: bool,
        owner: String,
        alias: String,
        body: Vec<Statement>,
    },
    Choice {
        options: Vec<ChoiceOption>,
    },
    Jump {
        scene: String,
    },
    Call {
        scene: String,
    },
    Return,
    Play {
        bus: AudioBus,
        asset: AssetRef,
        looping: bool,
        fade_ms: Option<u32>,
    },
    Stop {
        bus: AudioBus,
        fade_ms: Option<u32>,
    },
    Volume {
        bus: AudioBus,
        value: f64,
    },
    Save {
        slot: u32,
    },
    Load {
        slot: u32,
    },
    SetFlag {
        name: String,
        value: bool,
        persistent: bool,
    },
    SetTextSpeed {
        speed_ms: u32,
    },
    SetAuto {
        enabled: bool,
    },
    SetSkip {
        mode: String,
    },
    SetLocale {
        locale: String,
    },
    SetTheme {
        theme: String,
    },
    SetTextBox {
        bounds: RectSpec,
        color: String,
        opacity: u8,
        mode: String,
    },
    Tween {
        node: NodeAccess,
        property: String,
        value: f64,
        duration_ms: u32,
        easing: String,
    },
    Effect {
        kind: String,
        color: String,
        amount: f64,
        duration_ms: u32,
        axis: String,
    },
    UnlockChapter {
        id: String,
        progress: u8,
    },
    SetChapterProgress {
        id: String,
        progress: u8,
    },
    UnlockCg {
        id: String,
    },
    Preload {
        asset: AssetRef,
    },
    OpenMenu {
        kind: String,
    },
    /// Semantic screen routing. The named route is part of the presentation
    /// contract; it never creates a VM-drawn menu.
    OpenScreen {
        screen: String,
    },
    End,
}

/// The contents created by a `show` statement.
#[derive(Debug, Clone, PartialEq)]
pub enum ShowContent {
    Image {
        asset: AssetRef,
        position: Position,
    },
    Rect {
        bounds: RectSpec,
        color: String,
    },
    Text {
        text: String,
        position: Position,
        size_px: i32,
    },
}

/// An explicit scene-node borrow. Scene mutation operations require `&mut`;
/// a bare node name is intentionally never accepted as an implicit mutable
/// borrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAccess {
    pub span: SourceSpan,
    pub name: String,
    pub mutable: bool,
}

/// A project-relative asset reference.  Canonical path and hash validation are
/// intentionally semantic/package stages, not parser work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRef {
    pub span: SourceSpan,
    pub path: String,
}

/// A logical-pixel position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub span: SourceSpan,
    pub x_px: i32,
    pub y_px: i32,
}

/// A logical-pixel rectangle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RectSpec {
    pub span: SourceSpan,
    pub x_px: i32,
    pub y_px: i32,
    pub width_px: i32,
    pub height_px: i32,
}

/// A background transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub span: SourceSpan,
    pub kind: TransitionKind,
    /// `fade`, `fade_through_black`, and `wipe` may leave duration to a later
    /// theme/default stage.
    /// `mask` requires an explicit duration syntactically.
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Fade,
    /// A location change that goes through a dark field before the new
    /// photograph is revealed.  This is distinct from a short authored fade
    /// used by statements and colour grades.
    FadeThroughBlack,
    Wipe,
    Mask,
}

/// A selection option and its target scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    pub span: SourceSpan,
    pub text: String,
    pub scene: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBus {
    Bgm,
    Se,
    Voice,
}

/// A declaration/assignment right-hand side in the initial subset.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Literal(Literal),
    Identifier { span: SourceSpan, name: String },
}

impl Value {
    #[must_use]
    pub fn span(&self) -> &SourceSpan {
        match self {
            Self::Literal(literal) => literal.span(),
            Self::Identifier { span, .. } => span,
        }
    }
}

/// A literal accepted in state/declaration assignments and expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer { span: SourceSpan, value: i64 },
    Boolean { span: SourceSpan, value: bool },
    String { span: SourceSpan, value: String },
}

impl Literal {
    #[must_use]
    pub fn span(&self) -> &SourceSpan {
        match self {
            Self::Integer { span, .. } | Self::Boolean { span, .. } | Self::String { span, .. } => {
                span
            }
        }
    }

    #[must_use]
    pub fn ty(&self) -> ModernType {
        match self {
            Self::Integer { .. } => ModernType::Int,
            Self::Boolean { .. } => ModernType::Bool,
            Self::String { .. } => ModernType::String,
        }
    }
}

/// Boolean and comparison expressions used by `if` and `while`.
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub span: SourceSpan,
    pub kind: ExpressionKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExpressionKind {
    Literal(Literal),
    Identifier(String),
    Unary {
        op: UnaryOperator,
        expression: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        op: BinaryOperator,
        right: Box<Expression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Not,
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

/// Parses a modern Aria source file without touching the filesystem.
///
/// The function is deliberately recovery-oriented: malformed input produces
/// diagnostics and a best-effort AST rather than panicking.
#[must_use]
pub fn parse(source_id: impl Into<String>, source: impl Into<String>) -> ModernParse {
    let source_id = source_id.into();
    let source = source.into();
    let mut diagnostics = Vec::new();
    let tokens = Lexer::new(&source_id, &source).lex(&mut diagnostics);
    let cst = ModernCst {
        source_id,
        source,
        tokens,
    };
    let mut parser = Parser::new(&cst, diagnostics);
    let module = parser.parse_module();
    parser.diagnostics.sort_by(|left, right| {
        left.span
            .as_ref()
            .map(|span| (&span.source, span.line, span.column))
            .cmp(
                &right
                    .span
                    .as_ref()
                    .map(|span| (&span.source, span.line, span.column)),
            )
    });
    ModernParse {
        cst,
        module,
        diagnostics: parser.diagnostics,
    }
}

struct Lexer<'a> {
    source_id: &'a str,
    source: &'a str,
    offset: usize,
    line: u32,
    column: u32,
}

impl<'a> Lexer<'a> {
    fn new(source_id: &'a str, source: &'a str) -> Self {
        Self {
            source_id,
            source,
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    fn lex(mut self, diagnostics: &mut Vec<Diagnostic>) -> Vec<ModernToken> {
        let mut tokens = Vec::new();
        while self.offset < self.source.len() {
            let start = self.mark();
            let Some(character) = self.peek_char() else {
                break;
            };
            let kind = if character.is_whitespace() {
                self.consume_while(|next| next.is_whitespace());
                ModernTokenKind::Whitespace
            } else if self.starts_with("//") {
                self.advance_byte_pair();
                while self
                    .peek_char()
                    .is_some_and(|next| next != '\n' && next != '\r')
                {
                    self.advance_char();
                }
                ModernTokenKind::LineComment
            } else if self.starts_with("/*") {
                self.advance_byte_pair();
                let mut closed = false;
                while self.offset < self.source.len() {
                    if self.starts_with("*/") {
                        self.advance_byte_pair();
                        closed = true;
                        break;
                    }
                    self.advance_char();
                }
                if !closed {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidSyntax,
                        "unterminated block comment",
                        Some(self.span_from_mark(start)),
                    ));
                }
                ModernTokenKind::BlockComment
            } else if character == '"' {
                self.advance_char();
                let mut escaped = false;
                let mut closed = false;
                while let Some(next) = self.peek_char() {
                    self.advance_char();
                    if escaped {
                        escaped = false;
                    } else if next == '\\' {
                        escaped = true;
                    } else if next == '"' {
                        closed = true;
                        break;
                    } else if next == '\n' || next == '\r' {
                        break;
                    }
                }
                if !closed {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidSyntax,
                        "unterminated string literal",
                        Some(self.span_from_mark(start)),
                    ));
                }
                ModernTokenKind::String
            } else if is_identifier_start(character) {
                self.advance_char();
                self.consume_while(is_identifier_continue);
                ModernTokenKind::Identifier
            } else if character.is_ascii_digit() {
                self.consume_while(|next| next.is_ascii_digit());
                if self.peek_char() == Some('.') {
                    let after_dot = self.source[self.offset + 1..].chars().next();
                    if after_dot.is_some_and(|next| next.is_ascii_digit()) {
                        self.advance_char();
                        self.consume_while(|next| next.is_ascii_digit());
                    }
                }
                ModernTokenKind::Number
            } else {
                self.lex_punctuation(diagnostics, start)
            };
            tokens.push(self.token_from_mark(kind, start));
        }
        let eof = self.mark();
        tokens.push(ModernToken {
            kind: ModernTokenKind::Eof,
            text: String::new(),
            span: self.span_from_mark(eof),
            byte_start: eof.offset,
            byte_end: eof.offset,
        });
        tokens
    }

    fn lex_punctuation(
        &mut self,
        diagnostics: &mut Vec<Diagnostic>,
        start: Mark,
    ) -> ModernTokenKind {
        let pairs = [
            ("+=", ModernTokenKind::PlusEquals),
            ("=>", ModernTokenKind::FatArrow),
            ("==", ModernTokenKind::EqualEqual),
            ("!=", ModernTokenKind::BangEqual),
            ("<=", ModernTokenKind::LessEqual),
            (">=", ModernTokenKind::GreaterEqual),
            ("&&", ModernTokenKind::AndAnd),
            ("||", ModernTokenKind::OrOr),
        ];
        for (text, kind) in pairs {
            if self.starts_with(text) {
                self.advance_byte_pair();
                return kind;
            }
        }
        let kind = match self.advance_char() {
            Some(';') => ModernTokenKind::Semicolon,
            Some(':') => ModernTokenKind::Colon,
            Some(',') => ModernTokenKind::Comma,
            Some('.') => ModernTokenKind::Dot,
            Some('(') => ModernTokenKind::LeftParen,
            Some(')') => ModernTokenKind::RightParen,
            Some('{') => ModernTokenKind::LeftBrace,
            Some('}') => ModernTokenKind::RightBrace,
            Some('=') => ModernTokenKind::Equals,
            Some('+') => ModernTokenKind::Plus,
            Some('-') => ModernTokenKind::Minus,
            Some('!') => ModernTokenKind::Bang,
            Some('&') => ModernTokenKind::Ampersand,
            Some('<') => ModernTokenKind::Less,
            Some('>') => ModernTokenKind::Greater,
            Some(_) => ModernTokenKind::Invalid,
            None => ModernTokenKind::Eof,
        };
        if kind == ModernTokenKind::Invalid {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidSyntax,
                "unexpected character",
                Some(self.span_from_mark(start)),
            ));
        }
        kind
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }

    fn starts_with(&self, text: &str) -> bool {
        self.source[self.offset..].starts_with(text)
    }

    fn consume_while(&mut self, predicate: impl Fn(char) -> bool) {
        while self.peek_char().is_some_and(&predicate) {
            self.advance_char();
        }
    }

    fn advance_byte_pair(&mut self) {
        self.advance_char();
        self.advance_char();
    }

    fn advance_char(&mut self) -> Option<char> {
        if self.offset >= self.source.len() {
            return None;
        }
        if self.source[self.offset..].starts_with("\r\n") {
            self.offset += 2;
            self.line = self.line.saturating_add(1);
            self.column = 1;
            return Some('\n');
        }
        let character = self.peek_char()?;
        self.offset += character.len_utf8();
        if matches!(character, '\n' | '\r') {
            self.line = self.line.saturating_add(1);
            self.column = 1;
        } else {
            self.column = self.column.saturating_add(1);
        }
        Some(character)
    }

    fn mark(&self) -> Mark {
        Mark {
            offset: self.offset,
            line: self.line,
            column: self.column,
        }
    }

    fn span_from_mark(&self, mark: Mark) -> SourceSpan {
        SourceSpan {
            source: self.source_id.to_owned(),
            line: mark.line,
            column: mark.column,
            length: u32::try_from(self.offset.saturating_sub(mark.offset)).unwrap_or(u32::MAX),
        }
    }

    fn token_from_mark(&self, kind: ModernTokenKind, mark: Mark) -> ModernToken {
        ModernToken {
            kind,
            text: self.source[mark.offset..self.offset].to_owned(),
            span: self.span_from_mark(mark),
            byte_start: mark.offset,
            byte_end: self.offset,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Mark {
    offset: usize,
    line: u32,
    column: u32,
}

fn is_identifier_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

fn is_identifier_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

struct Parser {
    tokens: Vec<ModernToken>,
    eof: ModernToken,
    index: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn new(cst: &ModernCst, diagnostics: Vec<Diagnostic>) -> Self {
        let tokens = cst
            .tokens
            .iter()
            .filter(|token| !token.kind.is_trivia())
            .cloned()
            .collect::<Vec<_>>();
        let eof = tokens
            .iter()
            .rev()
            .find(|token| token.kind == ModernTokenKind::Eof)
            .cloned()
            .unwrap_or_else(|| ModernToken {
                kind: ModernTokenKind::Eof,
                text: String::new(),
                span: SourceSpan {
                    source: cst.source_id.clone(),
                    line: 1,
                    column: 1,
                    length: 0,
                },
                byte_start: cst.source.len(),
                byte_end: cst.source.len(),
            });
        Self {
            tokens,
            eof,
            index: 0,
            diagnostics,
        }
    }

    fn parse_module(&mut self) -> Option<ModernModule> {
        let header_start = self.peek().clone();
        if !self.consume_keyword("aria") {
            self.error_here("Aria source must start with the single language marker 'aria;'");
            return None;
        }
        if self.peek().kind == ModernTokenKind::Number {
            let version = self.next().expect("peeked token exists");
            self.error_at(
                &version,
                "Aria source is unversioned; write 'aria;' rather than a versioned language mode",
            );
        }
        let mut last = self.expect_kind(
            ModernTokenKind::Semicolon,
            "expected ';' after language header",
        );
        let mut module = ModernModule {
            span: header_start.span.clone(),
            name: None,
            imports: Vec::new(),
            entry: None,
            states: Vec::new(),
            scenes: Vec::new(),
            retired_ui_syntax: Vec::new(),
        };

        while !self.at_eof() {
            let before = self.index;
            let parsed_last = if self.consume_keyword("module") {
                let declaration = self.parse_module_name();
                if module.name.is_some() {
                    self.error_span(
                        &declaration.span,
                        "a source file may declare only one module",
                    );
                } else {
                    module.name = Some(declaration);
                }
                self.expect_kind(
                    ModernTokenKind::Semicolon,
                    "expected ';' after module declaration",
                )
            } else if self.consume_keyword("use") {
                let declaration = self.parse_import();
                if let Some(declaration) = declaration {
                    module.imports.push(declaration);
                }
                self.expect_kind(
                    ModernTokenKind::Semicolon,
                    "expected ';' after import declaration",
                )
            } else if self.consume_keyword("import") {
                let span = self.previous_or(&header_start).span.clone();
                self.error_span(
                    &span,
                    "'import' was removed; use Rust-style 'use \"path.aria\";'",
                );
                let declaration = self.parse_import();
                if let Some(declaration) = declaration {
                    module.imports.push(declaration);
                }
                self.expect_kind(
                    ModernTokenKind::Semicolon,
                    "expected ';' after import declaration",
                )
            } else if self.consume_keyword("entry") {
                let declaration = self.parse_entry();
                if let Some(declaration) = declaration {
                    if module.entry.is_some() {
                        self.error_span(
                            &declaration.span,
                            "a source file may declare only one entry scene",
                        );
                    } else {
                        module.entry = Some(declaration);
                    }
                }
                self.expect_kind(
                    ModernTokenKind::Semicolon,
                    "expected ';' after entry declaration",
                )
            } else if self.consume_keyword("state") {
                let declaration = self.parse_state();
                if let Some(declaration) = declaration {
                    module.states.push(declaration);
                }
                self.expect_kind(
                    ModernTokenKind::Semicolon,
                    "expected ';' after state declaration",
                )
            } else if self.consume_keyword("scene") {
                let declaration = self.parse_scene();
                let end = self.previous_or(&header_start).clone();
                if let Some(declaration) = declaration {
                    module.scenes.push(declaration);
                }
                Some(end)
            } else if self.consume_keyword("ui_theme") {
                if let Some(declaration) = self.parse_retired_ui_declaration(RetiredUiKind::Theme) {
                    module.retired_ui_syntax.push(declaration);
                }
                let end = self.previous_or(&header_start).clone();
                Some(end)
            } else if self.consume_keyword("ui_screen") {
                if let Some(declaration) = self.parse_retired_ui_declaration(RetiredUiKind::Screen)
                {
                    module.retired_ui_syntax.push(declaration);
                }
                let end = self.previous_or(&header_start).clone();
                Some(end)
            } else if self.consume_keyword("ui_transition") {
                if let Some(declaration) =
                    self.parse_retired_ui_declaration(RetiredUiKind::Transition)
                {
                    module.retired_ui_syntax.push(declaration);
                }
                let end = self.previous_or(&header_start).clone();
                Some(end)
            } else {
                self.error_here(
                    "expected module, use, entry, state, scene, ui_theme, ui_screen, or ui_transition declaration",
                );
                self.recover_top_level();
                None
            };
            if let Some(token) = parsed_last {
                last = Some(token);
            }
            if self.index == before {
                self.advance();
            }
        }

        // Whether an entry point is required depends on how this source is
        // reached. Imported modules deliberately have no entry declaration;
        // that policy belongs to semantic analysis, where the compiler knows
        // which source is the project entry. Keeping it out of the parser
        // also lets tools parse and format reusable modules in isolation.
        module.span = last.as_ref().map_or_else(
            || header_start.span.clone(),
            |end| span_between(&header_start, end),
        );
        Some(module)
    }

    fn parse_module_name(&mut self) -> QualifiedName {
        let start = self.peek().clone();
        let mut segments = Vec::new();
        match self.parse_identifier("expected a module name") {
            Some((name, _)) => segments.push(name),
            None => {
                return QualifiedName {
                    span: start.span,
                    segments,
                };
            }
        }
        let mut end = self.previous_or(&start).clone();
        while self.consume_kind(ModernTokenKind::Dot).is_some() {
            match self.parse_identifier("expected an identifier after '.' in module name") {
                Some((name, token)) => {
                    segments.push(name);
                    end = token;
                }
                None => break,
            }
        }
        QualifiedName {
            span: span_between(&start, &end),
            segments,
        }
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
        let token = self.expect_kind(ModernTokenKind::String, "expected import path string")?;
        let path = self.decode_string(&token)?;
        Some(ImportDecl {
            span: token.span,
            path,
        })
    }

    fn parse_entry(&mut self) -> Option<EntryDecl> {
        let (scene, token) = self.parse_identifier("expected entry scene name")?;
        Some(EntryDecl {
            span: token.span,
            scene,
        })
    }

    fn parse_state(&mut self) -> Option<StateDecl> {
        let start = self.peek().clone();
        let mutable = self.consume_keyword("mut");
        let (name, _) = self.parse_identifier("expected state name")?;
        self.expect_kind(ModernTokenKind::Colon, "expected ':' after state name")?;
        let ty = self.parse_type()?;
        self.expect_kind(ModernTokenKind::Equals, "expected '=' after state type")?;
        let value = self.parse_literal("expected Int, Bool, or String state literal")?;
        if value.ty() != ty {
            self.error_span(
                value.span(),
                "state initializer type must match the declared state type",
            );
        }
        Some(StateDecl {
            span: span_from_to(&start.span, value.span()),
            mutable,
            name,
            ty,
            value,
        })
    }

    fn parse_scene(&mut self) -> Option<SceneDecl> {
        let start = self.peek().clone();
        let (name, _) = self.parse_identifier("expected scene name after 'scene'")?;
        let body = self.parse_block("expected '{' to open scene body")?;
        let end = self.previous_or(&start).clone();
        Some(SceneDecl {
            span: span_between(&start, &end),
            name,
            body,
        })
    }

    /// Consumes retired visual DSL syntax without constructing a visual model.
    /// Keeping only a source span is intentional: no retired declaration can
    /// accidentally regain a lowering path while diagnostics remain precise
    /// and useful.
    fn parse_retired_ui_declaration(
        &mut self,
        kind: RetiredUiKind,
    ) -> Option<RetiredUiDeclaration> {
        let start = self
            .tokens
            .get(self.index.saturating_sub(1))
            .map(|token| token.span.clone())
            .unwrap_or_else(|| self.peek().span.clone());
        let requires_block = !matches!(kind, RetiredUiKind::Transition);
        let mut depth = 0_u32;
        let mut saw_block = false;
        let mut end = start.clone();

        while !self.at_eof() {
            let token = self.next()?;
            end = token.span.clone();
            match token.kind {
                ModernTokenKind::LeftBrace => {
                    saw_block = true;
                    depth = depth.saturating_add(1);
                }
                ModernTokenKind::RightBrace if depth > 0 => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && requires_block {
                        break;
                    }
                }
                ModernTokenKind::Semicolon if depth == 0 && !requires_block => break,
                ModernTokenKind::Semicolon if depth == 0 && requires_block && !saw_block => {
                    self.error_span(&start, "expected '{' in retired visual UI declaration");
                    break;
                }
                _ => {}
            }
        }

        if requires_block && (!saw_block || depth != 0) {
            self.error_span(&start, "unterminated retired visual UI declaration");
        }
        Some(RetiredUiDeclaration {
            kind,
            span: span_from_to(&start, &end),
        })
    }

    fn parse_block(&mut self, opening_message: &str) -> Option<Vec<Statement>> {
        self.expect_kind(ModernTokenKind::LeftBrace, opening_message)?;
        let mut statements = Vec::new();
        while !self.at_eof() && !self.at_kind(ModernTokenKind::RightBrace) {
            let before = self.index;
            if let Some(statement) = self.parse_statement() {
                statements.push(statement);
            } else {
                self.recover_statement();
            }
            if self.index == before {
                self.advance();
            }
        }
        self.expect_kind(ModernTokenKind::RightBrace, "expected '}' to close block")?;
        Some(statements)
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        let start = self.peek().clone();
        let kind = if self.consume_keyword("say") {
            self.parse_say()?
        } else if self.consume_keyword("narrate") {
            let text = self.parse_string("expected narration string")?;
            self.expect_statement_end("expected ';' after narrate statement")?;
            StatementKind::Narrate { text }
        } else if self.consume_keyword("clear") {
            self.expect_keyword("dialogue", "expected 'dialogue' after 'clear'")?;
            self.expect_statement_end("expected ';' after clear dialogue")?;
            StatementKind::ClearDialogue
        } else if self.consume_keyword("await") {
            self.expect_keyword("advance", "expected 'advance' after 'await'")?;
            self.expect_statement_end("expected ';' after await advance")?;
            StatementKind::AwaitAdvance
        } else if self.consume_keyword("wait") {
            let release_after_ms = if self.consume_keyword("breath") {
                Some(160)
            } else {
                None
            };
            let duration_ms = self.parse_duration_ms()?;
            self.expect_statement_end("expected ';' after wait statement")?;
            StatementKind::Wait {
                duration_ms,
                release_after_ms,
            }
        } else if self.consume_keyword("background") {
            self.parse_background()?
        } else if self.consume_keyword("hide") {
            let node = self.parse_node_access(true, "expected '&mut node' after hide")?;
            self.expect_statement_end("expected ';' after hide statement")?;
            StatementKind::Hide { node }
        } else if self.consume_keyword("reveal") {
            let node = self.parse_node_access(true, "expected '&mut node' after reveal")?;
            self.expect_statement_end("expected ';' after reveal statement")?;
            StatementKind::Reveal { node }
        } else if self.consume_keyword("drop") {
            let name = self.parse_identifier("expected owned node after drop")?.0;
            self.expect_statement_end("expected ';' after drop statement")?;
            StatementKind::Drop { name }
        } else if self.consume_keyword("move") {
            let node = self.parse_node_access(true, "expected '&mut node' after move")?;
            self.expect_keyword("to", "expected 'to' in move statement")?;
            let position = self.parse_position()?;
            self.expect_statement_end("expected ';' after move statement")?;
            StatementKind::Move { node, position }
        } else if self.consume_keyword("let") {
            self.parse_let_declaration()?
        } else if self.consume_keyword("var") {
            let span = self.previous_or(&start).span.clone();
            self.error_span(
                &span,
                "'var' was removed; write 'let mut' for a mutable binding",
            );
            self.parse_declaration(true)?
        } else if self.consume_keyword("if") {
            self.parse_if()?
        } else if self.consume_keyword("while") {
            self.parse_while()?
        } else if self.consume_keyword("borrow") {
            self.parse_borrow()?
        } else if self.consume_keyword("choice") {
            self.parse_choice()?
        } else if self.consume_keyword("jump") {
            let scene = self.parse_identifier("expected scene name after jump")?.0;
            self.expect_statement_end("expected ';' after jump statement")?;
            StatementKind::Jump { scene }
        } else if self.consume_keyword("call") {
            let scene = self.parse_identifier("expected scene name after call")?.0;
            self.expect_statement_end("expected ';' after call statement")?;
            StatementKind::Call { scene }
        } else if self.consume_keyword("return") {
            self.expect_statement_end("expected ';' after return statement")?;
            StatementKind::Return
        } else if self.consume_keyword("play") {
            self.parse_play()?
        } else if self.consume_keyword("stop") {
            self.parse_stop()?
        } else if self.consume_keyword("volume") {
            self.parse_volume()?
        } else if self.consume_keyword("flag") {
            self.parse_flag(false)?
        } else if self.consume_keyword("persistent") {
            self.expect_keyword("flag", "expected 'flag' after 'persistent'")?;
            self.parse_flag(true)?
        } else if self.consume_keyword("text_speed") {
            let speed_ms = self.parse_u32("expected non-negative text speed")?;
            self.expect_statement_end("expected ';' after text speed")?;
            StatementKind::SetTextSpeed { speed_ms }
        } else if self.consume_keyword("auto") {
            let enabled = self.parse_on_off("expected 'on' or 'off' after auto")?;
            self.expect_statement_end("expected ';' after auto statement")?;
            StatementKind::SetAuto { enabled }
        } else if self.consume_keyword("skip") {
            let mode = self.parse_word("expected skip mode read, all, or off")?;
            self.expect_statement_end("expected ';' after skip statement")?;
            StatementKind::SetSkip { mode }
        } else if self.consume_keyword("locale") {
            let locale = self.parse_string("expected locale string")?;
            self.expect_statement_end("expected ';' after locale statement")?;
            StatementKind::SetLocale { locale }
        } else if self.consume_keyword("theme") {
            let theme = if self.at_kind(ModernTokenKind::String) {
                self.parse_string("expected theme name")?
            } else {
                self.parse_word("expected theme name")?
            };
            self.expect_statement_end("expected ';' after theme statement")?;
            StatementKind::SetTheme { theme }
        } else if self.consume_keyword("textbox") {
            self.parse_textbox()?
        } else if self.consume_keyword("tween") {
            self.parse_tween()?
        } else if self.consume_keyword("effect") {
            self.parse_effect()?
        } else if self.consume_keyword("unlock") {
            self.parse_unlock()?
        } else if self.consume_keyword("chapter") || self.consume_keyword("chapter_progress") {
            self.parse_chapter_progress()?
        } else if self.consume_keyword("preload") {
            let asset = self.parse_asset_ref()?;
            self.expect_statement_end("expected ';' after preload statement")?;
            StatementKind::Preload { asset }
        } else if self.consume_keyword("menu") {
            let kind = if self.at_kind(ModernTokenKind::Semicolon) {
                "pause".to_owned()
            } else {
                self.parse_word("expected menu kind")?
            };
            self.expect_statement_end("expected ';' after menu statement")?;
            StatementKind::OpenMenu { kind }
        } else if self.consume_keyword("open") {
            let kind = self.parse_word("expected UI name after open")?;
            self.expect_statement_end("expected ';' after open statement")?;
            StatementKind::OpenMenu { kind }
        } else if self.consume_keyword("screen") {
            let screen = self.parse_word("expected screen name after screen")?;
            self.expect_statement_end("expected ';' after screen statement")?;
            StatementKind::OpenScreen { screen }
        } else if self.consume_keyword("save") {
            let slot = self.parse_u32("expected non-negative save slot")?;
            self.expect_statement_end("expected ';' after save statement")?;
            StatementKind::Save { slot }
        } else if self.consume_keyword("load") {
            let slot = self.parse_u32("expected non-negative load slot")?;
            self.expect_statement_end("expected ';' after load statement")?;
            StatementKind::Load { slot }
        } else if self.consume_keyword("end") {
            self.expect_statement_end("expected ';' after end statement")?;
            StatementKind::End
        } else if self.peek().kind == ModernTokenKind::Identifier {
            self.parse_assignment()?
        } else {
            self.error_here("expected a modern Aria statement");
            return None;
        };
        let end = self.previous_or(&start).clone();
        Some(Statement {
            span: span_between(&start, &end),
            kind,
        })
    }

    fn parse_say(&mut self) -> Option<StatementKind> {
        let speaker = if self.at_kind(ModernTokenKind::String) {
            None
        } else {
            let (name, _) = self.parse_identifier("expected speaker name or dialogue string")?;
            self.expect_kind(ModernTokenKind::Colon, "expected ':' after speaker name")?;
            Some(name)
        };
        let text = self.parse_string("expected dialogue string")?;
        self.expect_statement_end("expected ';' after say statement")?;
        Some(StatementKind::Say { speaker, text })
    }

    fn parse_background(&mut self) -> Option<StatementKind> {
        let asset = self.parse_asset_ref()?;
        let transition = if self.consume_keyword("with") {
            Some(self.parse_transition()?)
        } else {
            None
        };
        self.expect_statement_end("expected ';' after background statement")?;
        Some(StatementKind::Background { asset, transition })
    }

    fn parse_transition(&mut self) -> Option<Transition> {
        let token = self.next()?;
        let kind = match token.text.as_str() {
            "fade" => TransitionKind::Fade,
            "fade_through_black" | "fade-through-black" => TransitionKind::FadeThroughBlack,
            "wipe" => TransitionKind::Wipe,
            "mask" => TransitionKind::Mask,
            _ => {
                self.error_at(
                    &token,
                    "expected fade, fade_through_black, wipe, or mask transition",
                );
                return None;
            }
        };
        let duration_ms = if self.consume_kind(ModernTokenKind::LeftParen).is_some() {
            let duration = self.parse_duration_ms()?;
            self.expect_kind(
                ModernTokenKind::RightParen,
                "expected ')' after transition duration",
            )?;
            Some(duration)
        } else {
            None
        };
        if kind == TransitionKind::Mask && duration_ms.is_none() {
            self.error_at(&token, "mask transition requires '(durationms)'");
        }
        let end = self.previous_or(&token).clone();
        Some(Transition {
            span: span_between(&token, &end),
            kind,
            duration_ms,
        })
    }

    fn parse_show(&mut self, mutable: bool, name: String) -> Option<StatementKind> {
        let constructor = self.next()?;
        let content = match constructor.text.as_str() {
            "image" => {
                self.expect_kind(ModernTokenKind::LeftParen, "expected '(' after image")?;
                let asset = self.parse_asset_ref()?;
                self.expect_kind(
                    ModernTokenKind::RightParen,
                    "expected ')' after image asset",
                )?;
                self.expect_keyword("at", "expected 'at' after image constructor")?;
                let position = self.parse_position()?;
                ShowContent::Image { asset, position }
            }
            "rect" => {
                self.expect_kind(ModernTokenKind::LeftParen, "expected '(' after rect")?;
                let rect_start = self.previous_or(&constructor).clone();
                let x_px = self.parse_px()?;
                self.expect_kind(ModernTokenKind::Comma, "expected ',' after rect x")?;
                let y_px = self.parse_px()?;
                self.expect_kind(ModernTokenKind::Comma, "expected ',' after rect y")?;
                let width_px = self.parse_px()?;
                self.expect_kind(ModernTokenKind::Comma, "expected ',' after rect width")?;
                let height_px = self.parse_px()?;
                self.expect_kind(ModernTokenKind::Comma, "expected ',' after rect height")?;
                let color = self.parse_string("expected rect color string")?;
                let close =
                    self.expect_kind(ModernTokenKind::RightParen, "expected ')' after rect")?;
                ShowContent::Rect {
                    bounds: RectSpec {
                        span: span_between(&rect_start, &close),
                        x_px,
                        y_px,
                        width_px,
                        height_px,
                    },
                    color,
                }
            }
            "text" => {
                self.expect_kind(ModernTokenKind::LeftParen, "expected '(' after text")?;
                let text = self.parse_string("expected text content string")?;
                self.expect_kind(
                    ModernTokenKind::RightParen,
                    "expected ')' after text content",
                )?;
                self.expect_keyword("at", "expected 'at' after text constructor")?;
                let position = self.parse_position()?;
                self.expect_keyword("size", "expected 'size' after text position")?;
                let size_px = self.parse_px()?;
                ShowContent::Text {
                    text,
                    position,
                    size_px,
                }
            }
            _ => {
                self.error_at(
                    &constructor,
                    "expected image, rect, or text show constructor",
                );
                return None;
            }
        };
        self.expect_keyword("z", "expected 'z' after show content")?;
        let z = self.parse_i32("expected integer z value")?;
        self.expect_statement_end("expected ';' after show statement")?;
        Some(StatementKind::Spawn {
            mutable,
            name,
            content,
            z,
        })
    }

    fn parse_let_declaration(&mut self) -> Option<StatementKind> {
        let mutable = self.consume_keyword("mut");
        let (name, _) = self.parse_identifier("expected variable name after let")?;
        let ty = if self.consume_kind(ModernTokenKind::Colon).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_kind(ModernTokenKind::Equals, "expected '=' after let binding")?;
        if self.consume_keyword("show") {
            if ty.is_some_and(|ty| ty != ModernType::Node) {
                self.error_here(
                    "a 'show' expression creates a Node; omit the type or write ': Node'",
                );
            }
            return self.parse_show(mutable, name);
        }
        let value = self.parse_value("expected a literal or binding after '='")?;
        self.expect_statement_end("expected ';' after let declaration")?;
        Some(StatementKind::Declare {
            mutable,
            name,
            ty,
            value,
        })
    }

    /// Kept only for a precise diagnostic and parser recovery after the
    /// removed `var` keyword. A successful parse still carries an error.
    fn parse_declaration(&mut self, mutable: bool) -> Option<StatementKind> {
        let (name, _) = self.parse_identifier("expected variable name")?;
        let ty = if self.consume_kind(ModernTokenKind::Colon).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_kind(ModernTokenKind::Equals, "expected '=' after variable name")?;
        let value = self.parse_value("expected a literal or identifier")?;
        self.expect_statement_end("expected ';' after variable declaration")?;
        Some(StatementKind::Declare {
            mutable,
            name,
            ty,
            value,
        })
    }

    fn parse_borrow(&mut self) -> Option<StatementKind> {
        let mutable = self.consume_keyword("mut");
        let (owner, _) = self.parse_identifier("expected owned node after borrow")?;
        self.expect_keyword("as", "expected 'as' after borrowed node")?;
        let (alias, _) = self.parse_identifier("expected borrow alias after 'as'")?;
        let body = self.parse_block("expected '{' after borrow alias")?;
        Some(StatementKind::Borrow {
            mutable,
            owner,
            alias,
            body,
        })
    }

    fn parse_node_access(&mut self, require_mutable: bool, message: &str) -> Option<NodeAccess> {
        let start = self.expect_kind(ModernTokenKind::Ampersand, message)?;
        let mutable = self.consume_keyword("mut");
        if require_mutable && !mutable {
            self.error_span(
                &start.span,
                "scene mutation requires an explicit mutable borrow '&mut node'",
            );
        }
        let (name, end) = self.parse_identifier("expected node name after '&'")?;
        Some(NodeAccess {
            span: span_between(&start, &end),
            name,
            mutable,
        })
    }

    fn parse_assignment(&mut self) -> Option<StatementKind> {
        let (name, _) = self.parse_identifier("expected assignment target")?;
        if self.consume_kind(ModernTokenKind::Equals).is_some() {
            let value = self.parse_value("expected a literal or identifier after '='")?;
            self.expect_statement_end("expected ';' after assignment")?;
            Some(StatementKind::Assign { name, value })
        } else if self.consume_kind(ModernTokenKind::PlusEquals).is_some() {
            let value = self.parse_i64("expected integer after '+='")?;
            self.expect_statement_end("expected ';' after '+=' assignment")?;
            Some(StatementKind::AddAssign { name, value })
        } else {
            self.error_here("expected '=' or '+=' after assignment target");
            None
        }
    }

    fn parse_if(&mut self) -> Option<StatementKind> {
        let condition = self.parse_expression(0)?;
        let then_branch = self.parse_block("expected '{' after if condition")?;
        let else_branch = if self.consume_keyword("else") {
            if self.consume_keyword("if") {
                let nested = self.parse_if()?;
                let span = expression_span(&condition);
                vec![Statement { span, kind: nested }]
            } else {
                self.parse_block("expected '{' after else")?
            }
        } else {
            Vec::new()
        };
        Some(StatementKind::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn parse_while(&mut self) -> Option<StatementKind> {
        let condition = self.parse_expression(0)?;
        let body = self.parse_block("expected '{' after while condition")?;
        Some(StatementKind::While { condition, body })
    }

    fn parse_choice(&mut self) -> Option<StatementKind> {
        self.expect_kind(ModernTokenKind::LeftBrace, "expected '{' after choice")?;
        let mut options = Vec::new();
        while !self.at_eof() && !self.at_kind(ModernTokenKind::RightBrace) {
            let start = self.peek().clone();
            let text = self.parse_string("expected choice option string")?;
            self.expect_kind(ModernTokenKind::FatArrow, "expected '=>' after choice text")?;
            let (scene, scene_token) = self.parse_identifier("expected choice target scene")?;
            let end = self.expect_kind(
                ModernTokenKind::Semicolon,
                "expected ';' after choice option",
            )?;
            options.push(ChoiceOption {
                span: span_between(&start, &end),
                text,
                scene,
            });
            let _ = scene_token;
        }
        self.expect_kind(
            ModernTokenKind::RightBrace,
            "expected '}' after choice options",
        )?;
        if options.is_empty() {
            self.error_here("choice requires at least one option");
        }
        // A trailing semicolon is harmless and makes formatting of nested
        // constructs pleasant, while the canonical formatter may omit it.
        self.consume_kind(ModernTokenKind::Semicolon);
        Some(StatementKind::Choice { options })
    }

    fn parse_play(&mut self) -> Option<StatementKind> {
        let bus = self.parse_audio_bus()?;
        let asset = self.parse_asset_ref()?;
        let mut looping = false;
        let mut fade_ms = None;
        while !self.at_eof() && !self.at_kind(ModernTokenKind::Semicolon) {
            if self.consume_keyword("loop") {
                if looping {
                    self.error_here("play statement may specify 'loop' only once");
                }
                looping = true;
            } else if self.consume_keyword("fade") {
                if fade_ms.is_some() {
                    self.error_here("play statement may specify fade only once");
                }
                fade_ms = Some(self.parse_duration_ms()?);
            } else {
                self.error_here("expected 'loop', 'fade <duration>ms', or ';' after play asset");
                self.advance();
            }
        }
        self.expect_statement_end("expected ';' after play statement")?;
        Some(StatementKind::Play {
            bus,
            asset,
            looping,
            fade_ms,
        })
    }

    fn parse_stop(&mut self) -> Option<StatementKind> {
        let bus = self.parse_audio_bus()?;
        let fade_ms = if self.consume_keyword("fade") {
            Some(self.parse_duration_ms()?)
        } else {
            None
        };
        self.expect_statement_end("expected ';' after stop statement")?;
        Some(StatementKind::Stop { bus, fade_ms })
    }

    fn parse_volume(&mut self) -> Option<StatementKind> {
        let bus = self.parse_audio_bus()?;
        let value = self.parse_f64("expected numeric volume")?;
        self.expect_statement_end("expected ';' after volume statement")?;
        Some(StatementKind::Volume { bus, value })
    }

    fn parse_flag(&mut self, persistent: bool) -> Option<StatementKind> {
        let name = self.parse_string("expected flag name string")?;
        let _ = self.consume_kind(ModernTokenKind::Equals);
        let value = self.parse_on_off("expected boolean flag value")?;
        self.expect_statement_end("expected ';' after flag statement")?;
        Some(StatementKind::SetFlag {
            name,
            value,
            persistent,
        })
    }

    fn parse_textbox(&mut self) -> Option<StatementKind> {
        let start = self.expect_kind(
            ModernTokenKind::LeftParen,
            "expected '(' before textbox bounds",
        )?;
        let x_px = self.parse_px()?;
        self.expect_kind(ModernTokenKind::Comma, "expected ',' after textbox x")?;
        let y_px = self.parse_px()?;
        self.expect_kind(ModernTokenKind::Comma, "expected ',' after textbox y")?;
        let width_px = self.parse_px()?;
        self.expect_kind(ModernTokenKind::Comma, "expected ',' after textbox width")?;
        let height_px = self.parse_px()?;
        let end = self.expect_kind(
            ModernTokenKind::RightParen,
            "expected ')' after textbox bounds",
        )?;
        self.expect_keyword("color", "expected 'color' after textbox bounds")?;
        let color = self.parse_string("expected textbox color")?;
        self.expect_keyword("opacity", "expected 'opacity' after textbox color")?;
        let opacity = self.parse_u32("expected textbox opacity")?.min(255) as u8;
        let mode = if self.consume_keyword("mode") {
            self.parse_word("expected textbox mode adv or nvl")?
        } else {
            "adv".to_owned()
        };
        self.expect_statement_end("expected ';' after textbox statement")?;
        Some(StatementKind::SetTextBox {
            bounds: RectSpec {
                span: span_between(&start, &end),
                x_px,
                y_px,
                width_px,
                height_px,
            },
            color,
            opacity,
            mode,
        })
    }

    fn parse_tween(&mut self) -> Option<StatementKind> {
        let node = self.parse_node_access(true, "expected '&mut node' after tween")?;
        self.expect_keyword("property", "expected 'property' after tween node")?;
        let property = self.parse_string("expected tween property")?;
        self.expect_keyword("to", "expected 'to' in tween statement")?;
        let value = self.parse_f64("expected tween target value")?;
        self.expect_keyword("over", "expected 'over' in tween statement")?;
        let duration_ms = self.parse_duration_ms()?;
        let easing = if self.consume_keyword("ease") {
            self.parse_word("expected easing name")?
        } else {
            "linear".to_owned()
        };
        self.expect_statement_end("expected ';' after tween statement")?;
        Some(StatementKind::Tween {
            node,
            property,
            value,
            duration_ms,
            easing,
        })
    }

    fn parse_effect(&mut self) -> Option<StatementKind> {
        let kind = self.parse_word("expected effect kind")?;
        let color = self.parse_string("expected effect color")?;
        self.expect_keyword("amount", "expected 'amount' after effect color")?;
        let amount = self.parse_f64("expected effect amount")?;
        self.expect_keyword("over", "expected 'over' in effect statement")?;
        let duration_ms = self.parse_duration_ms()?;
        let axis = if self.consume_keyword("axis") {
            self.parse_word("expected effect axis")?
        } else {
            String::new()
        };
        self.expect_statement_end("expected ';' after effect statement")?;
        Some(StatementKind::Effect {
            kind,
            color,
            amount,
            duration_ms,
            axis,
        })
    }

    fn parse_unlock(&mut self) -> Option<StatementKind> {
        if self.consume_keyword("cg") {
            let id = self.parse_string("expected CG ID")?;
            self.expect_statement_end("expected ';' after CG unlock")?;
            return Some(StatementKind::UnlockCg { id });
        }
        self.expect_keyword("chapter", "expected 'chapter' or 'cg' after unlock")?;
        let id = self.parse_string("expected chapter ID")?;
        let progress = if self.consume_keyword("progress") {
            self.parse_u32("expected chapter progress")?.min(100) as u8
        } else {
            0
        };
        self.expect_statement_end("expected ';' after chapter unlock")?;
        Some(StatementKind::UnlockChapter { id, progress })
    }

    fn parse_chapter_progress(&mut self) -> Option<StatementKind> {
        let id = self.parse_string("expected chapter ID")?;
        self.expect_keyword("progress", "expected 'progress' after chapter ID")?;
        let progress = self.parse_u32("expected chapter progress")?.min(100) as u8;
        self.expect_statement_end("expected ';' after chapter progress")?;
        Some(StatementKind::SetChapterProgress { id, progress })
    }

    fn parse_word(&mut self, message: &str) -> Option<String> {
        self.parse_identifier(message).map(|(name, _)| name)
    }

    fn parse_on_off(&mut self, message: &str) -> Option<bool> {
        let word = self.parse_word(message)?;
        match word.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Some(true),
            "off" | "false" | "0" => Some(false),
            _ => {
                self.error_here(message);
                None
            }
        }
    }

    fn parse_asset_ref(&mut self) -> Option<AssetRef> {
        let start = self.peek().clone();
        self.expect_keyword("asset", "expected asset(\"path\")")?;
        self.expect_kind(ModernTokenKind::LeftParen, "expected '(' after asset")?;
        let string = self.expect_kind(ModernTokenKind::String, "expected asset path string")?;
        let path = self.decode_string(&string)?;
        let end = self.expect_kind(ModernTokenKind::RightParen, "expected ')' after asset path")?;
        Some(AssetRef {
            span: span_between(&start, &end),
            path,
        })
    }

    fn parse_position(&mut self) -> Option<Position> {
        let start = self.expect_kind(ModernTokenKind::LeftParen, "expected '(' before position")?;
        let x_px = self.parse_px()?;
        self.expect_kind(
            ModernTokenKind::Comma,
            "expected ',' between position coordinates",
        )?;
        let y_px = self.parse_px()?;
        let end = self.expect_kind(ModernTokenKind::RightParen, "expected ')' after position")?;
        Some(Position {
            span: span_between(&start, &end),
            x_px,
            y_px,
        })
    }

    fn parse_type(&mut self) -> Option<ModernType> {
        let token = self.next()?;
        match token.text.as_str() {
            "Int" => Some(ModernType::Int),
            "Bool" => Some(ModernType::Bool),
            "String" => Some(ModernType::String),
            "Node" => Some(ModernType::Node),
            _ => {
                self.error_at(&token, "expected Int, Bool, String, or Node type");
                None
            }
        }
    }

    fn parse_value(&mut self, message: &str) -> Option<Value> {
        if let Some(literal) = self.parse_literal_if_present()? {
            return Some(Value::Literal(literal));
        }
        let (name, token) = self.parse_identifier(message)?;
        Some(Value::Identifier {
            span: token.span,
            name,
        })
    }

    fn parse_literal(&mut self, message: &str) -> Option<Literal> {
        match self.parse_literal_if_present()? {
            Some(literal) => Some(literal),
            None => {
                self.error_here(message);
                None
            }
        }
    }

    fn parse_literal_if_present(&mut self) -> Option<Option<Literal>> {
        let token = self.peek().clone();
        if token.kind == ModernTokenKind::String {
            self.advance();
            return Some(Some(Literal::String {
                span: token.span.clone(),
                value: self.decode_string(&token)?,
            }));
        }
        if token.kind == ModernTokenKind::Number {
            self.advance();
            return Some(Some(Literal::Integer {
                span: token.span.clone(),
                value: self.parse_integer_token(&token)?,
            }));
        }
        if token.kind == ModernTokenKind::Minus
            && self
                .peek_next()
                .is_some_and(|next| next.kind == ModernTokenKind::Number)
        {
            let start = self.next()?;
            let number = self.next()?;
            let value = self.parse_integer_token(&number)?;
            return Some(Some(Literal::Integer {
                span: span_between(&start, &number),
                value: value.saturating_neg(),
            }));
        }
        if token.kind == ModernTokenKind::Identifier
            && matches!(token.text.as_str(), "true" | "false")
        {
            self.advance();
            return Some(Some(Literal::Boolean {
                span: token.span,
                value: token.text == "true",
            }));
        }
        Some(None)
    }

    fn parse_expression(&mut self, min_precedence: u8) -> Option<Expression> {
        let mut left = self.parse_unary()?;
        while let Some((operator, precedence)) = binary_operator(self.peek().kind) {
            if precedence < min_precedence {
                break;
            }
            self.advance();
            let right = self.parse_expression(precedence.saturating_add(1))?;
            let span = span_from_to(&left.span, &right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    left: Box::new(left),
                    op: operator,
                    right: Box::new(right),
                },
            };
        }
        Some(left)
    }

    fn parse_unary(&mut self) -> Option<Expression> {
        if let Some(token) = self.consume_kind(ModernTokenKind::Bang) {
            let expression = self.parse_unary()?;
            return Some(Expression {
                span: span_from_to(&token.span, &expression.span),
                kind: ExpressionKind::Unary {
                    op: UnaryOperator::Not,
                    expression: Box::new(expression),
                },
            });
        }
        if let Some(token) = self.consume_kind(ModernTokenKind::Minus) {
            let expression = self.parse_unary()?;
            return Some(Expression {
                span: span_from_to(&token.span, &expression.span),
                kind: ExpressionKind::Unary {
                    op: UnaryOperator::Negate,
                    expression: Box::new(expression),
                },
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Option<Expression> {
        if let Some(open) = self.consume_kind(ModernTokenKind::LeftParen) {
            let mut expression = self.parse_expression(0)?;
            let close =
                self.expect_kind(ModernTokenKind::RightParen, "expected ')' after expression")?;
            expression.span = span_between(&open, &close);
            return Some(expression);
        }
        if let Some(literal) = self.parse_literal_if_present()? {
            let span = literal.span().clone();
            return Some(Expression {
                span,
                kind: ExpressionKind::Literal(literal),
            });
        }
        let (name, token) = self.parse_identifier("expected expression")?;
        Some(Expression {
            span: token.span,
            kind: ExpressionKind::Identifier(name),
        })
    }

    fn parse_audio_bus(&mut self) -> Option<AudioBus> {
        let token = self.next()?;
        match token.text.as_str() {
            "bgm" => Some(AudioBus::Bgm),
            "se" => Some(AudioBus::Se),
            "voice" => Some(AudioBus::Voice),
            _ => {
                self.error_at(&token, "expected audio bus bgm, se, or voice");
                None
            }
        }
    }

    fn parse_duration_ms(&mut self) -> Option<u32> {
        let value = self.parse_u32("expected non-negative duration in milliseconds")?;
        self.expect_keyword("ms", "expected 'ms' after duration")?;
        Some(value)
    }

    fn parse_px(&mut self) -> Option<i32> {
        let value = self.parse_i32("expected logical pixel integer")?;
        self.expect_keyword("px", "expected 'px' after logical pixel value")?;
        Some(value)
    }

    fn parse_i32(&mut self, message: &str) -> Option<i32> {
        let value = self.parse_i64(message)?;
        match i32::try_from(value) {
            Ok(value) => Some(value),
            Err(_) => {
                self.error_here("integer is outside the supported i32 range");
                None
            }
        }
    }

    fn parse_u32(&mut self, message: &str) -> Option<u32> {
        let token = self.peek().clone();
        if token.kind == ModernTokenKind::Minus {
            self.error_at(&token, message);
            self.advance();
            return None;
        }
        let value = self.parse_i64(message)?;
        match u32::try_from(value) {
            Ok(value) => Some(value),
            Err(_) => {
                self.error_at(&token, message);
                None
            }
        }
    }

    fn parse_i64(&mut self, message: &str) -> Option<i64> {
        let minus = self.consume_kind(ModernTokenKind::Minus);
        let token = self.expect_kind(ModernTokenKind::Number, message)?;
        let value = self.parse_integer_token(&token)?;
        Some(if minus.is_some() {
            value.saturating_neg()
        } else {
            value
        })
    }

    fn parse_f64(&mut self, message: &str) -> Option<f64> {
        let minus = self.consume_kind(ModernTokenKind::Minus);
        let token = self.expect_kind(ModernTokenKind::Number, message)?;
        let value = match token.text.parse::<f64>() {
            Ok(value) if value.is_finite() => value,
            _ => {
                self.error_at(&token, message);
                return None;
            }
        };
        Some(if minus.is_some() { -value } else { value })
    }

    fn parse_integer_token(&mut self, token: &ModernToken) -> Option<i64> {
        if token.text.contains('.') {
            self.error_at(token, "expected integer without a decimal point");
            return None;
        }
        match token.text.parse::<i64>() {
            Ok(value) => Some(value),
            Err(_) => {
                self.error_at(token, "integer literal is outside the supported i64 range");
                None
            }
        }
    }

    fn parse_string(&mut self, message: &str) -> Option<String> {
        let token = self.expect_kind(ModernTokenKind::String, message)?;
        self.decode_string(&token)
    }

    fn decode_string(&mut self, token: &ModernToken) -> Option<String> {
        let raw = token.text.as_str();
        if raw.len() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
            self.error_at(token, "invalid string literal");
            return None;
        }
        let content = &raw[1..raw.len() - 1];
        let mut output = String::with_capacity(content.len());
        let mut chars = content.chars();
        while let Some(character) = chars.next() {
            if character != '\\' {
                output.push(character);
                continue;
            }
            let Some(escaped) = chars.next() else {
                self.error_at(token, "unterminated string escape");
                return None;
            };
            match escaped {
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                '"' => output.push('"'),
                '\\' => output.push('\\'),
                other => {
                    self.error_at(token, format!("unsupported string escape '\\{other}'"));
                    return None;
                }
            }
        }
        Some(output)
    }

    fn parse_identifier(&mut self, message: &str) -> Option<(String, ModernToken)> {
        let token = self.expect_kind(ModernTokenKind::Identifier, message)?;
        Some((token.text.clone(), token))
    }

    fn expect_keyword(&mut self, keyword: &str, message: &str) -> Option<ModernToken> {
        if self.peek().kind == ModernTokenKind::Identifier && self.peek().text == keyword {
            return self.next();
        }
        self.error_here(message);
        None
    }

    fn expect_statement_end(&mut self, message: &str) -> Option<ModernToken> {
        self.expect_kind(ModernTokenKind::Semicolon, message)
    }

    fn expect_kind(&mut self, kind: ModernTokenKind, message: &str) -> Option<ModernToken> {
        if self.peek().kind == kind {
            self.next()
        } else {
            self.error_here(message);
            None
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        if self.peek().kind == ModernTokenKind::Identifier && self.peek().text == keyword {
            self.advance();
            true
        } else {
            false
        }
    }

    fn consume_kind(&mut self, kind: ModernTokenKind) -> Option<ModernToken> {
        if self.peek().kind == kind {
            self.next()
        } else {
            None
        }
    }

    fn at_kind(&self, kind: ModernTokenKind) -> bool {
        self.peek().kind == kind
    }

    fn at_eof(&self) -> bool {
        self.peek().kind == ModernTokenKind::Eof
    }

    fn peek(&self) -> &ModernToken {
        // The lexer supplies an EOF token.  The owned fallback still makes
        // recovery safe if a future caller constructs a CST manually.
        self.tokens.get(self.index).unwrap_or(&self.eof)
    }

    fn peek_next(&self) -> Option<&ModernToken> {
        self.tokens.get(self.index.saturating_add(1))
    }

    fn previous_or<'b>(&'b self, fallback: &'b ModernToken) -> &'b ModernToken {
        self.index
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .unwrap_or(fallback)
    }

    fn next(&mut self) -> Option<ModernToken> {
        if self.at_eof() {
            return None;
        }
        let token = self.peek().clone();
        self.index = self.index.saturating_add(1);
        Some(token)
    }

    fn advance(&mut self) {
        if !self.at_eof() {
            self.index = self.index.saturating_add(1);
        }
    }

    fn error_here(&mut self, message: impl Into<String>) {
        let token = self.peek().clone();
        self.error_at(&token, message);
    }

    fn error_at(&mut self, token: &ModernToken, message: impl Into<String>) {
        self.error_span(&token.span, message);
    }

    fn error_span(&mut self, span: &SourceSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidSyntax,
            message,
            Some(span.clone()),
        ));
    }

    fn recover_top_level(&mut self) {
        while !self.at_eof() {
            if self.consume_kind(ModernTokenKind::Semicolon).is_some() {
                break;
            }
            if self.peek().kind == ModernTokenKind::Identifier
                && matches!(
                    self.peek().text.as_str(),
                    "module"
                        | "use"
                        | "import"
                        | "entry"
                        | "state"
                        | "scene"
                        | "ui_theme"
                        | "ui_screen"
                        | "ui_transition"
                )
            {
                break;
            }
            self.advance();
        }
    }

    fn recover_statement(&mut self) {
        let mut depth = 0_u32;
        while !self.at_eof() {
            match self.peek().kind {
                ModernTokenKind::LeftBrace => {
                    depth = depth.saturating_add(1);
                    self.advance();
                }
                ModernTokenKind::RightBrace if depth == 0 => break,
                ModernTokenKind::RightBrace => {
                    depth = depth.saturating_sub(1);
                    self.advance();
                }
                ModernTokenKind::Semicolon if depth == 0 => {
                    self.advance();
                    break;
                }
                _ => self.advance(),
            }
        }
    }
}

fn binary_operator(kind: ModernTokenKind) -> Option<(BinaryOperator, u8)> {
    match kind {
        ModernTokenKind::OrOr => Some((BinaryOperator::Or, 1)),
        ModernTokenKind::AndAnd => Some((BinaryOperator::And, 2)),
        ModernTokenKind::EqualEqual => Some((BinaryOperator::Equal, 3)),
        ModernTokenKind::BangEqual => Some((BinaryOperator::NotEqual, 3)),
        ModernTokenKind::Less => Some((BinaryOperator::Less, 4)),
        ModernTokenKind::LessEqual => Some((BinaryOperator::LessEqual, 4)),
        ModernTokenKind::Greater => Some((BinaryOperator::Greater, 4)),
        ModernTokenKind::GreaterEqual => Some((BinaryOperator::GreaterEqual, 4)),
        _ => None,
    }
}

fn span_between(start: &ModernToken, end: &ModernToken) -> SourceSpan {
    SourceSpan {
        source: start.span.source.clone(),
        line: start.span.line,
        column: start.span.column,
        length: u32::try_from(end.byte_end.saturating_sub(start.byte_start)).unwrap_or(u32::MAX),
    }
}

fn span_from_to(start: &SourceSpan, end: &SourceSpan) -> SourceSpan {
    let length = if start.source == end.source && start.line == end.line {
        end.column
            .saturating_add(end.length)
            .saturating_sub(start.column)
    } else {
        start.length
    };
    SourceSpan {
        source: start.source.clone(),
        line: start.line,
        column: start.column,
        length,
    }
}

fn expression_span(expression: &Expression) -> SourceSpan {
    expression.span.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_structured_japanese_scene_and_preserves_source() {
        let source = "// 海風\r\n\
aria;\r\n\
module 海風.main;\r\n\
use \"./common.aria\";\r\n\
entry start;\r\n\
state route: Int = 0;\r\n\
scene start {\r\n\
  background asset(\"#07131f\") with fade(300ms);\r\n\
  let mut ミオ = show image(asset(\"ch/mio.webp\")) at (760px, 86px) z 20;\r\n\
  say ミオ: \"海へ行こう。\";\r\n\
  choice { \"海へ行く\" => sea; \"駅へ戻る\" => station; }\r\n\
}\r\n";
        let parsed = parse("scripts/main.aria", source);
        assert!(!parsed.has_errors(), "{:#?}", parsed.diagnostics);
        assert_eq!(parsed.cst.lossless_source(), source);
        let module = parsed.module.unwrap();
        assert_eq!(module.name.unwrap().as_string(), "海風.main");
        assert_eq!(module.entry.unwrap().scene, "start");
        assert_eq!(module.scenes[0].name, "start");
        assert!(matches!(
            module.scenes[0].body[2].kind,
            StatementKind::Say { ref speaker, ref text }
                if speaker.as_deref() == Some("ミオ") && text == "海へ行こう。"
        ));
    }

    #[test]
    fn parses_all_show_forms_audio_and_control_flow() {
        let source = "aria;\n\
entry start;\n\
scene start {\n\
  let mut panel = show rect(0px, 0px, 1280px, 720px, \"#001122\") z -1;\n\
  let mut title = show text(\"海風\") at (40px, 60px) size 32px z 2;\n\
  let mut seen: Bool = false;\n\
  if !seen || true { play bgm asset(\"audio/sea.ogg\") loop fade 250ms; } else { stop bgm fade 10ms; }\n\
  while seen == false { seen = true; }\n\
  volume bgm 0.75;\n\
  move &mut title to (80px, 60px);\n\
  end;\n\
}\n";
        let parsed = parse("main.aria", source);
        assert!(!parsed.has_errors(), "{:#?}", parsed.diagnostics);
        let body = &parsed.module.unwrap().scenes[0].body;
        assert!(matches!(body[0].kind, StatementKind::Spawn { .. }));
        assert!(matches!(body[3].kind, StatementKind::If { .. }));
        assert!(
            matches!(body[5].kind, StatementKind::Volume { value, .. } if (value - 0.75).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn parses_breath_waits_and_fade_through_black_as_first_class_story_syntax() {
        let parsed = parse(
            "main.aria",
            "aria;\nentry start;\nscene start {\n\
             background asset(\"bg/scenes/platform.webp\") with fade_through_black(640ms);\n\
             wait breath 300ms;\n\
             wait 220ms;\n\
             end;\n}\n",
        );
        assert!(!parsed.has_errors(), "{:#?}", parsed.diagnostics);
        let body = &parsed.module.unwrap().scenes[0].body;
        assert!(matches!(
            body[0].kind,
            StatementKind::Background {
                transition: Some(Transition {
                    kind: TransitionKind::FadeThroughBlack,
                    duration_ms: Some(640),
                    ..
                }),
                ..
            }
        ));
        assert!(matches!(
            body[1].kind,
            StatementKind::Wait {
                duration_ms: 300,
                release_after_ms: Some(160),
            }
        ));
        assert!(matches!(
            body[2].kind,
            StatementKind::Wait {
                duration_ms: 220,
                release_after_ms: None,
            }
        ));
    }

    #[test]
    fn reports_errors_without_panicking_or_emitting_a_module_for_missing_header() {
        let missing_header = parse("bad.aria", "scene start { say \"x\"; }");
        assert!(missing_header.module.is_none());
        assert!(missing_header.has_errors());

        let malformed = parse(
            "bad.aria",
            "aria;\nentry start;\nscene start { state nope: Int = \"wrong\"; say \"unterminated\n",
        );
        assert!(malformed.has_errors());
        assert!(
            malformed
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::InvalidSyntax)
        );
    }

    #[test]
    fn rejects_language_modes_and_compatibility_declarations_with_actionable_errors() {
        let parsed = parse(
            "bad.aria",
            "aria 3.2;\nimport \"common.aria\";\nentry start;\nscene start { var count: Int = 0; end; }\n",
        );
        assert!(parsed.has_errors());
        let messages = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("unversioned"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("Rust-style 'use"))
        );
        assert!(messages.iter().any(|message| message.contains("let mut")));
    }

    #[test]
    fn parses_an_importable_module_without_an_entry_point() {
        let parsed = parse(
            "scripts/common.aria",
            "aria;\nmodule example.common;\nstate met: Bool = false;\nscene greet { return; }\n",
        );
        assert!(!parsed.has_errors(), "{:#?}", parsed.diagnostics);
        let module = parsed.module.unwrap();
        assert!(module.entry.is_none());
        assert_eq!(module.scenes[0].name, "greet");
    }
}
