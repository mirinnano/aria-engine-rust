use std::collections::{BTreeMap, BTreeSet};

use crate::bytecode::{
    ARIAC_FORMAT_VERSION, ByteOp, CompiledProgram, Constant, EncodedInstruction, LanguageVersion,
    Operand, SourceLocation,
};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
use crate::syntax::{CommandSyntax, SyntaxKind, SyntaxTree, unquote};
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

    if sources
        .get(&entry)
        .is_some_and(|source| is_modern_source(source))
    {
        return crate::modern_compiler::compile_modern(input.game_id, entry, sources, diagnostics);
    }

    let functions = collect_function_names(&sources);
    let mut compiler = Compiler {
        game_id: input.game_id,
        entry: entry.clone(),
        sources,
        functions,
        included: BTreeSet::new(),
        active_includes: Vec::new(),
        constants: Vec::new(),
        string_constants: BTreeMap::new(),
        instructions: Vec::new(),
        source_map: Vec::new(),
        labels: BTreeMap::new(),
        references: Vec::new(),
        blocks: Vec::new(),
        generated_label: 0,
        unsupported_reported: BTreeSet::new(),
        diagnostics,
    };
    compiler.compile_source(&entry, true);
    compiler.finish()
}

#[derive(Debug)]
struct Compiler {
    game_id: String,
    entry: String,
    sources: BTreeMap<String, String>,
    functions: BTreeSet<String>,
    included: BTreeSet<String>,
    active_includes: Vec<String>,
    constants: Vec<Constant>,
    string_constants: BTreeMap<String, u32>,
    instructions: Vec<EncodedInstruction>,
    source_map: Vec<SourceLocation>,
    labels: BTreeMap<String, u32>,
    references: Vec<LabelReference>,
    blocks: Vec<Block>,
    generated_label: u64,
    unsupported_reported: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct LabelReference {
    instruction: usize,
    operand: usize,
    label: String,
    span: SourceSpan,
}

#[derive(Debug)]
enum Block {
    If {
        false_label: String,
        end_label: String,
        saw_else: bool,
        span: SourceSpan,
    },
    While {
        start_label: String,
        end_label: String,
        span: SourceSpan,
    },
    Function {
        end_label: String,
        span: SourceSpan,
    },
}

impl Compiler {
    fn compile_source(&mut self, path: &str, require_version: bool) {
        if self.included.contains(path) {
            return;
        }
        if self.active_includes.iter().any(|active| active == path) {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidControlFlow,
                format!(
                    "include cycle: {} -> {path}",
                    self.active_includes.join(" -> ")
                ),
                None,
            ));
            return;
        }
        let Some(source) = self.sources.get(path).cloned() else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::MissingSource,
                format!("included source '{path}' is missing"),
                None,
            ));
            return;
        };

        self.active_includes.push(path.to_owned());
        self.included.insert(path.to_owned());
        let tree = SyntaxTree::parse(path, source);
        self.diagnostics.extend(tree.diagnostics.iter().cloned());
        self.check_language_version(&tree, require_version);

        for line in &tree.lines {
            let span = SourceSpan::line(path, line.line, line.raw.trim_end().len());
            match &line.kind {
                SyntaxKind::Empty | SyntaxKind::Comment | SyntaxKind::Directive { .. } => {}
                SyntaxKind::Label(label) => self.define_label(label, &span),
                SyntaxKind::Assignment {
                    target,
                    operator,
                    value,
                } => self.compile_line_assignment(target, operator, value, &span),
                SyntaxKind::Dialogue { speaker, content } => {
                    let speaker = speaker
                        .as_ref()
                        .map(|value| Operand::Constant(self.intern_string(value)))
                        .unwrap_or(Operand::None);
                    let content = Operand::Constant(self.intern_string(content));
                    self.emit(ByteOp::Text, vec![speaker, content], &span);
                }
                SyntaxKind::Advance { clear_page } => {
                    self.emit(
                        ByteOp::WaitAdvance,
                        vec![Operand::Boolean(*clear_page)],
                        &span,
                    );
                }
                SyntaxKind::Command(command) => self.compile_command(path, command, &span),
            }
        }
        self.active_includes.pop();
    }

    fn check_language_version(&mut self, tree: &SyntaxTree, required: bool) {
        let version = tree.lines.iter().find_map(|line| match &line.kind {
            SyntaxKind::Directive { name, value }
                if matches!(name.as_str(), "aria-version" | "aria_version") =>
            {
                Some((value.as_str(), line.line))
            }
            _ => None,
        });
        match version {
            Some((version, _)) if version.trim().starts_with("3.0") => {}
            Some((version, line)) => self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnsupportedLanguageVersion,
                format!(
                    "V3 compiler accepts only '# aria-version: 3.0'; found '{version}'. Run 'aria migrate'."
                ),
                Some(SourceSpan::line(tree.source_id(), line, version.len())),
            )),
            None if required => self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnsupportedLanguageVersion,
                "entry script must declare '# aria-version: 3.0'",
                Some(SourceSpan::line(tree.source_id(), 1, 1)),
            )),
            None => {}
        }
    }

    fn compile_command(&mut self, source_path: &str, command: &CommandSyntax, span: &SourceSpan) {
        let name = canonical_command(&command.name);
        match name.as_str() {
            "include" | "use" => self.compile_include(source_path, command, span),
            "strict" | "compat_mode" | "debug" | "caption" | "window" | "font"
            | "font_atlas_size" | "font_filter" | "script" => {}
            "func" => self.begin_function(command, span),
            "endfunc" => self.end_function(span),
            "defsub" | "getparam" => {}
            "goto" | "jmp" => {
                if let Some(label) = command.arguments.first() {
                    self.emit_label_reference(ByteOp::Jump, Vec::new(), label, span);
                } else {
                    self.invalid_operands(&name, "expected a label", span);
                }
            }
            "gosub" => {
                if let Some(label) = command.arguments.first() {
                    self.emit_label_reference(ByteOp::Call, Vec::new(), label, span);
                } else {
                    self.invalid_operands(&name, "expected a label", span);
                }
            }
            "return" => {
                self.emit(ByteOp::Return, Vec::new(), span);
            }
            "if" => self.begin_if(command, span),
            "else" => self.begin_else(span),
            "endif" => self.end_if(span),
            "while" => self.begin_while(command, span),
            "wend" => self.end_while(span),
            "text" => {
                if let Some(text) = command.arguments.first() {
                    let text = self.parse_operand(text);
                    self.emit(ByteOp::Text, vec![Operand::None, text], span);
                } else {
                    self.invalid_operands(&name, "expected text", span);
                }
            }
            "say" => self.compile_say(command, span),
            "await" => {
                if command
                    .arguments
                    .first()
                    .is_some_and(|argument| unquote(argument).eq_ignore_ascii_case("advance"))
                {
                    self.emit(ByteOp::WaitAdvance, vec![Operand::Boolean(false)], span);
                } else {
                    self.invalid_operands("await", "expected 'advance'", span);
                }
            }
            "textclear" | "erasetextwindow" => {
                self.emit(ByteOp::TextClear, Vec::new(), span);
            }
            "waitclick" | "wait_click" => {
                self.emit(ByteOp::WaitAdvance, vec![Operand::Boolean(false)], span);
            }
            "wait" => {
                let duration = command
                    .arguments
                    .first()
                    .map(|value| self.parse_operand(value))
                    .unwrap_or(Operand::Integer(0));
                self.emit(ByteOp::Delay, vec![duration], span);
            }
            "bg" | "loadbg" | "load_bg" => self.compile_background(command, span),
            "transition" => self.compile_transition(command, span),
            "lsp" | "loadch" | "load_ch" => self.compile_sprite_image(command, span),
            "lsp_text" | "ui_text" => self.compile_sprite_text(command, span),
            "lsp_rect" | "ui_rect" => self.compile_sprite_rect(command, span),
            "csp" | "clr" | "hidech" | "hide_ch" => {
                let id = command
                    .arguments
                    .first()
                    .map(|value| self.parse_operand(value))
                    .unwrap_or(Operand::Integer(-1));
                self.emit(ByteOp::SpriteRemove, vec![id], span);
            }
            "vsp" | "showch" | "show_ch" => self.compile_sprite_visibility(command, span),
            "msp" | "charmove" | "char_move" => self.compile_sprite_move(command, span),
            "choice" => self.compile_choice(command, span),
            "let" | "mov" => self.compile_assignment(command, span),
            "add" => self.compile_add(command, 1, span),
            "sub" => self.compile_add(command, -1, span),
            "inc" => self.compile_increment(command, 1, span),
            "dec" => self.compile_increment(command, -1, span),
            "playbgm" | "play_bgm" | "bgm" | "playmp3" => {
                self.compile_audio_play("bgm", true, command, span)
            }
            "dwave" | "playse" | "play_se" => {
                self.compile_audio_play("sound_effect", false, command, span)
            }
            "dwaveloop" => self.compile_audio_play("sound_effect", true, command, span),
            "voice" => self.compile_audio_play("voice", false, command, span),
            "stopbgm" | "stop_bgm" | "mp3fadeout" => self.compile_audio_stop("bgm", command, span),
            "dwavestop" | "stopse" | "stop_se" => {
                self.compile_audio_stop("sound_effect", command, span)
            }
            "voice_stop" | "voicestop" => self.compile_audio_stop("voice", command, span),
            "bgmvol" | "bgm_vol" => self.compile_volume("bgm", command, span),
            "sevol" | "se_vol" => self.compile_volume("sound_effect", command, span),
            "save" => self.compile_slot(ByteOp::Save, command, span),
            "load" => self.compile_slot(ByteOp::Load, command, span),
            "end" | "quit" => {
                self.emit(ByteOp::End, Vec::new(), span);
            }
            _ if self.functions.contains(&name) => {
                self.emit_label_reference(
                    ByteOp::Call,
                    Vec::new(),
                    &format!("__function_{name}"),
                    span,
                );
            }
            _ => self.compile_host(&name, command, span),
        }
    }

    fn compile_include(&mut self, source_path: &str, command: &CommandSyntax, span: &SourceSpan) {
        let Some(argument) = command.arguments.first() else {
            self.invalid_operands(&command.name, "expected a logical source path", span);
            return;
        };
        let requested = unquote(argument);
        match resolve_logical_path(source_path, &requested) {
            Ok(path) => self.compile_source(&path, false),
            Err(message) => self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::MissingSource,
                message,
                Some(span.clone()),
            )),
        }
    }

    fn begin_function(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        let Some(name) = function_name(command) else {
            self.invalid_operands("func", "expected a function name", span);
            return;
        };
        let end_label = self.fresh_label("function_end");
        self.emit_label_reference(ByteOp::Jump, Vec::new(), &end_label, span);
        self.define_label(&format!("__function_{name}"), span);
        self.blocks.push(Block::Function {
            end_label,
            span: span.clone(),
        });
    }

    fn end_function(&mut self, span: &SourceSpan) {
        let Some(Block::Function { end_label, .. }) = self.blocks.pop() else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidControlFlow,
                "endfunc without matching func",
                Some(span.clone()),
            ));
            return;
        };
        self.emit(ByteOp::Return, Vec::new(), span);
        self.define_label(&end_label, span);
    }

    fn begin_if(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        let (condition, inline) = split_inline_if(&command.raw_arguments);
        let Some(mut operands) = self.compile_condition(condition, span) else {
            return;
        };
        let false_label = self.fresh_label("if_false");
        let end_label = self.fresh_label("if_end");
        self.emit_label_reference(
            ByteOp::JumpIfFalse,
            operands.split_off(0),
            &false_label,
            span,
        );
        if let Some(inline) = inline {
            let synthetic = SyntaxTree::parse(span.source.clone(), inline.to_owned());
            if let Some(line) = synthetic.lines.first()
                && let SyntaxKind::Command(command) = &line.kind
            {
                self.compile_command(&span.source, command, span);
            } else {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidSyntax,
                    "inline if must contain a command",
                    Some(span.clone()),
                ));
            }
            self.define_label(&false_label, span);
        } else {
            self.blocks.push(Block::If {
                false_label,
                end_label,
                saw_else: false,
                span: span.clone(),
            });
        }
    }

    fn begin_else(&mut self, span: &SourceSpan) {
        let Some(Block::If {
            false_label,
            end_label,
            saw_else,
            ..
        }) = self.blocks.pop()
        else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidControlFlow,
                "else without matching if",
                Some(span.clone()),
            ));
            return;
        };
        if saw_else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidControlFlow,
                "if block contains more than one else",
                Some(span.clone()),
            ));
        }
        self.emit_label_reference(ByteOp::Jump, Vec::new(), &end_label, span);
        self.define_label(&false_label, span);
        self.blocks.push(Block::If {
            false_label,
            end_label,
            saw_else: true,
            span: span.clone(),
        });
    }

    fn end_if(&mut self, span: &SourceSpan) {
        let Some(Block::If {
            false_label,
            end_label,
            saw_else,
            ..
        }) = self.blocks.pop()
        else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidControlFlow,
                "endif without matching if",
                Some(span.clone()),
            ));
            return;
        };
        if saw_else {
            self.define_label(&end_label, span);
        } else {
            self.define_label(&false_label, span);
        }
    }

    fn begin_while(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        let start_label = self.fresh_label("while_start");
        let end_label = self.fresh_label("while_end");
        self.define_label(&start_label, span);
        let Some(operands) = self.compile_condition(&command.raw_arguments, span) else {
            return;
        };
        self.emit_label_reference(ByteOp::JumpIfFalse, operands, &end_label, span);
        self.blocks.push(Block::While {
            start_label,
            end_label,
            span: span.clone(),
        });
    }

    fn end_while(&mut self, span: &SourceSpan) {
        let Some(Block::While {
            start_label,
            end_label,
            ..
        }) = self.blocks.pop()
        else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidControlFlow,
                "wend without matching while",
                Some(span.clone()),
            ));
            return;
        };
        self.emit_label_reference(ByteOp::Jump, Vec::new(), &start_label, span);
        self.define_label(&end_label, span);
    }

    fn compile_condition(&mut self, raw: &str, span: &SourceSpan) -> Option<Vec<Operand>> {
        let tokens = tokenize_expression(raw);
        let (left, comparator, right) = match tokens.as_slice() {
            [left] => (left.as_str(), "truthy", "1"),
            [left, comparator, right] => (left.as_str(), comparator.as_str(), right.as_str()),
            _ => {
                self.invalid_operands("condition", "expected '<value> <comparison> <value>'", span);
                return None;
            }
        };
        if !matches!(
            comparator,
            "truthy" | "==" | "=" | "!=" | ">" | ">=" | "<" | "<="
        ) {
            self.invalid_operands("condition", "unsupported comparison operator", span);
            return None;
        }
        let comparator = Operand::Constant(self.intern_string(comparator));
        Some(vec![
            self.parse_operand(left),
            comparator,
            self.parse_operand(right),
        ])
    }

    fn compile_background(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        let Some(asset) = command.arguments.first() else {
            self.invalid_operands(&command.name, "expected an asset path or color", span);
            return;
        };
        let duration = command
            .arguments
            .get(1)
            .map(|value| self.parse_operand(value))
            .unwrap_or(Operand::Integer(0));
        let asset = self.parse_operand(asset);
        self.emit(ByteOp::Background, vec![asset, duration], span);
    }

    fn compile_transition(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        if command.arguments.len() < 2 {
            self.invalid_operands("transition", "expected target and asset", span);
            return;
        }
        let asset_index = usize::from(command.arguments[0].eq_ignore_ascii_case("bg"));
        let Some(asset) = command.arguments.get(asset_index) else {
            self.invalid_operands("transition", "expected an asset", span);
            return;
        };
        let kind = command
            .arguments
            .get(asset_index + 1)
            .map(|value| self.parse_operand(value))
            .unwrap_or_else(|| Operand::Constant(self.intern_string("fade")));
        let duration = command
            .arguments
            .get(asset_index + 2)
            .map(|value| self.parse_operand(value))
            .unwrap_or(Operand::Integer(300));
        let asset = self.parse_operand(asset);
        self.emit(ByteOp::Background, vec![asset, Operand::Integer(0)], span);
        self.emit(ByteOp::BeginTransition, vec![kind, duration], span);
    }

    fn compile_sprite_image(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        if command.arguments.len() < 2 {
            self.invalid_operands(&command.name, "expected id and asset", span);
            return;
        }
        let operands = padded_operands(self, &command.arguments, 6, [0, 0, 0, 255]);
        self.emit(ByteOp::SpriteImage, operands, span);
    }

    fn compile_sprite_text(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        if command.arguments.len() < 2 {
            self.invalid_operands(&command.name, "expected id and text", span);
            return;
        }
        let operands = padded_operands(self, &command.arguments, 6, [0, 0, 24, 0]);
        self.emit(ByteOp::SpriteText, operands, span);
    }

    fn compile_sprite_rect(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        if command.arguments.len() < 5 {
            self.invalid_operands(&command.name, "expected id, x, y, width, and height", span);
            return;
        }
        let mut operands = command
            .arguments
            .iter()
            .take(5)
            .map(|value| self.parse_operand(value))
            .collect::<Vec<_>>();
        operands.push(
            command
                .arguments
                .get(5)
                .map(|value| self.parse_operand(value))
                .unwrap_or_else(|| Operand::Constant(self.intern_string("#000000"))),
        );
        operands.push(
            command
                .arguments
                .get(6)
                .map(|value| self.parse_operand(value))
                .unwrap_or(Operand::Integer(0)),
        );
        self.emit(ByteOp::SpriteRect, operands, span);
    }

    fn compile_sprite_visibility(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        let Some(id) = command.arguments.first() else {
            self.invalid_operands(&command.name, "expected a sprite id", span);
            return;
        };
        let visible = command
            .arguments
            .get(1)
            .map(|value| self.parse_operand(value))
            .unwrap_or(Operand::Boolean(true));
        let id = self.parse_operand(id);
        self.emit(ByteOp::SpriteVisibility, vec![id, visible], span);
    }

    fn compile_sprite_move(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        if command.arguments.len() < 3 {
            self.invalid_operands(&command.name, "expected id, x, and y", span);
            return;
        }
        let operands = command
            .arguments
            .iter()
            .take(3)
            .map(|value| self.parse_operand(value))
            .collect();
        self.emit(ByteOp::SpriteMove, operands, span);
    }

    fn compile_choice(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        if command.arguments.len() < 2 || !command.arguments.len().is_multiple_of(2) {
            self.invalid_operands("choice", "expected text/label pairs", span);
            return;
        }
        let mut operands = Vec::with_capacity(command.arguments.len());
        for pair in command.arguments.chunks_exact(2) {
            operands.push(self.parse_operand(&pair[0]));
            let operand_index = operands.len();
            operands.push(Operand::Address(0));
            self.references.push(LabelReference {
                instruction: self.instructions.len(),
                operand: operand_index,
                label: clean_label(&pair[1]),
                span: span.clone(),
            });
        }
        self.emit(ByteOp::PresentChoice, operands, span);
    }

    fn compile_assignment(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        if command.arguments.len() < 2 {
            self.invalid_operands(&command.name, "expected target and value", span);
            return;
        }
        let target = self.parse_operand(&command.arguments[0]);
        let value = self.parse_operand(&command.arguments[1]);
        let op = if matches!(target, Operand::StringRegister(_)) {
            ByteOp::SetString
        } else {
            ByteOp::SetInt
        };
        self.emit(op, vec![target, value], span);
    }

    fn compile_line_assignment(
        &mut self,
        target: &str,
        operator: &str,
        value: &str,
        span: &SourceSpan,
    ) {
        let target = self.parse_operand(target);
        let value = self.parse_operand(value);
        if operator == "+=" {
            self.emit(
                ByteOp::AddInt,
                vec![target, value, Operand::Integer(1)],
                span,
            );
        } else {
            let op = if matches!(target, Operand::StringRegister(_)) {
                ByteOp::SetString
            } else {
                ByteOp::SetInt
            };
            self.emit(op, vec![target, value], span);
        }
    }

    fn compile_say(&mut self, command: &CommandSyntax, span: &SourceSpan) {
        let (speaker, text) = match command.arguments.as_slice() {
            [text] => (Operand::None, self.parse_operand(text)),
            [speaker, text] => (self.parse_operand(speaker), self.parse_operand(text)),
            _ => {
                self.invalid_operands("say", "expected text or speaker, text", span);
                return;
            }
        };
        self.emit(ByteOp::Text, vec![speaker, text], span);
    }

    fn compile_add(&mut self, command: &CommandSyntax, sign: i64, span: &SourceSpan) {
        if command.arguments.len() < 2 {
            self.invalid_operands(&command.name, "expected target and value", span);
            return;
        }
        let target = self.parse_operand(&command.arguments[0]);
        let value = self.parse_operand(&command.arguments[1]);
        self.emit(
            ByteOp::AddInt,
            vec![target, value, Operand::Integer(sign)],
            span,
        );
    }

    fn compile_increment(&mut self, command: &CommandSyntax, amount: i64, span: &SourceSpan) {
        let Some(target) = command.arguments.first() else {
            self.invalid_operands(&command.name, "expected a register", span);
            return;
        };
        let target = self.parse_operand(target);
        self.emit(
            ByteOp::AddInt,
            vec![target, Operand::Integer(amount), Operand::Integer(1)],
            span,
        );
    }

    fn compile_audio_play(
        &mut self,
        bus: &str,
        looping: bool,
        command: &CommandSyntax,
        span: &SourceSpan,
    ) {
        let Some(asset) = command.arguments.last() else {
            self.invalid_operands(&command.name, "expected an audio asset", span);
            return;
        };
        let id = if bus == "sound_effect" && command.arguments.len() > 1 {
            self.parse_operand(&command.arguments[0])
        } else {
            Operand::Constant(self.intern_string(bus))
        };
        let bus = Operand::Constant(self.intern_string(bus));
        let asset = self.parse_operand(asset);
        self.emit(
            ByteOp::PlayAudio,
            vec![
                bus,
                id,
                asset,
                Operand::Boolean(looping),
                Operand::Float(1.0),
                Operand::Integer(0),
            ],
            span,
        );
    }

    fn compile_audio_stop(&mut self, bus: &str, command: &CommandSyntax, span: &SourceSpan) {
        let bus = Operand::Constant(self.intern_string(bus));
        let id = command
            .arguments
            .first()
            .map(|value| self.parse_operand(value))
            .unwrap_or(Operand::None);
        let fade = command
            .arguments
            .last()
            .filter(|_| !command.arguments.is_empty())
            .map(|value| self.parse_operand(value))
            .unwrap_or(Operand::Integer(0));
        self.emit(ByteOp::StopAudio, vec![bus, id, fade], span);
    }

    fn compile_volume(&mut self, bus: &str, command: &CommandSyntax, span: &SourceSpan) {
        let Some(volume) = command.arguments.first() else {
            self.invalid_operands(&command.name, "expected volume", span);
            return;
        };
        let bus = Operand::Constant(self.intern_string(bus));
        let volume = self.parse_operand(volume);
        self.emit(
            ByteOp::SetVolume,
            vec![bus, volume, Operand::Integer(0)],
            span,
        );
    }

    fn compile_slot(&mut self, op: ByteOp, command: &CommandSyntax, span: &SourceSpan) {
        let slot = command
            .arguments
            .first()
            .map(|value| self.parse_operand(value))
            .unwrap_or(Operand::Integer(0));
        self.emit(op, vec![slot], span);
    }

    fn compile_host(&mut self, name: &str, command: &CommandSyntax, span: &SourceSpan) {
        if self.unsupported_reported.insert(name.to_owned()) {
            self.diagnostics.push(Diagnostic::warning(
                DiagnosticCode::UnsupportedRuntimeCommand,
                format!(
                    "'{name}' is preserved in V3 bytecode but its runtime semantics are not in the vertical slice"
                ),
                Some(span.clone()),
            ));
        }
        let name = Operand::Constant(self.intern_string(name));
        let arguments = Operand::Constant(self.intern_string(&command.raw_arguments));
        self.emit(ByteOp::Host, vec![name, arguments], span);
    }

    fn parse_operand(&mut self, raw: &str) -> Operand {
        let value = raw.trim();
        if let Some(register) = value.strip_prefix('%') {
            return Operand::IntRegister(register.to_owned());
        }
        if let Some(register) = value.strip_prefix('$') {
            return Operand::StringRegister(register.to_owned());
        }
        if let Ok(integer) = value.parse::<i64>() {
            return Operand::Integer(integer);
        }
        if let Ok(float) = value.parse::<f32>() {
            return Operand::Float(float);
        }
        if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("on") {
            return Operand::Boolean(true);
        }
        if value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("off") {
            return Operand::Boolean(false);
        }
        Operand::Constant(self.intern_string(&unquote(value)))
    }

    fn emit(&mut self, op: ByteOp, operands: Vec<Operand>, span: &SourceSpan) {
        self.instructions
            .push(EncodedInstruction::new(op, operands));
        self.source_map.push(SourceLocation {
            source: span.source.clone(),
            line: span.line,
            column: span.column,
        });
    }

    fn emit_label_reference(
        &mut self,
        op: ByteOp,
        mut operands: Vec<Operand>,
        label: &str,
        span: &SourceSpan,
    ) {
        let operand = operands.len();
        operands.push(Operand::Address(0));
        self.references.push(LabelReference {
            instruction: self.instructions.len(),
            operand,
            label: clean_label(label),
            span: span.clone(),
        });
        self.emit(op, operands, span);
    }

    fn define_label(&mut self, label: &str, span: &SourceSpan) {
        let label = clean_label(label);
        let address = u32::try_from(self.instructions.len()).unwrap_or(u32::MAX);
        if self.labels.insert(label.clone(), address).is_some() {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::DuplicateLabel,
                format!("duplicate label '*{label}'"),
                Some(span.clone()),
            ));
        }
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let value = format!("__generated_{prefix}_{}", self.generated_label);
        self.generated_label += 1;
        value
    }

    fn intern_string(&mut self, value: &str) -> u32 {
        if let Some(index) = self.string_constants.get(value) {
            return *index;
        }
        let index = u32::try_from(self.constants.len()).unwrap_or(u32::MAX);
        self.constants.push(Constant::String(value.to_owned()));
        self.string_constants.insert(value.to_owned(), index);
        index
    }

    fn invalid_operands(&mut self, command: &str, message: &str, span: &SourceSpan) {
        self.diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidOperand,
            format!("{command}: {message}"),
            Some(span.clone()),
        ));
    }

    fn finish(mut self) -> CompileOutput {
        if !self.blocks.is_empty() {
            for block in &self.blocks {
                let (name, span) = match block {
                    Block::If { span, .. } => ("if", span),
                    Block::While { span, .. } => ("while", span),
                    Block::Function { span, .. } => ("func", span),
                };
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidControlFlow,
                    format!("unclosed {name} block"),
                    Some(span.clone()),
                ));
            }
        }
        if self
            .instructions
            .last()
            .is_none_or(|instruction| instruction.op != ByteOp::End)
        {
            let span = SourceSpan::line(self.entry.clone(), 1, 1);
            self.emit(ByteOp::End, Vec::new(), &span);
        }

        for reference in self.references {
            let Some(address) = self.labels.get(&reference.label).copied() else {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnknownLabel,
                    format!("unknown label '*{}'", reference.label),
                    Some(reference.span),
                ));
                continue;
            };
            if let Some(operand) = self
                .instructions
                .get_mut(reference.instruction)
                .and_then(|instruction| instruction.operands.get_mut(reference.operand))
            {
                *operand = Operand::Address(address);
            }
        }

        let has_errors = self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error);
        self.diagnostics.sort_by(|left, right| {
            left.span
                .as_ref()
                .map(|span| (&span.source, span.line, span.column))
                .cmp(
                    &right
                        .span
                        .as_ref()
                        .map(|span| (&span.source, span.line, span.column)),
                )
                .then_with(|| left.code.cmp(&right.code))
        });
        let program = (!has_errors).then_some(CompiledProgram {
            format_version: ARIAC_FORMAT_VERSION,
            language_version: LanguageVersion::V3,
            game_id: self.game_id,
            constants: self.constants,
            instructions: self.instructions,
            source_map: self.source_map,
        });
        CompileOutput {
            program,
            diagnostics: self.diagnostics,
        }
    }
}

fn padded_operands<const N: usize>(
    compiler: &mut Compiler,
    arguments: &[String],
    total: usize,
    defaults: [i64; N],
) -> Vec<Operand> {
    let mut operands = arguments
        .iter()
        .take(total)
        .map(|value| compiler.parse_operand(value))
        .collect::<Vec<_>>();
    for default in defaults.into_iter().skip(operands.len().saturating_sub(2)) {
        if operands.len() >= total {
            break;
        }
        operands.push(Operand::Integer(default));
    }
    while operands.len() < total {
        operands.push(Operand::Integer(0));
    }
    operands
}

fn canonical_command(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

fn clean_label(label: &str) -> String {
    label.trim().trim_start_matches('*').to_ascii_lowercase()
}

fn function_name(command: &CommandSyntax) -> Option<String> {
    let raw = command.raw_arguments.trim();
    let name = raw
        .split(|character: char| character.is_whitespace() || character == '(')
        .next()?
        .trim();
    (!name.is_empty()).then(|| canonical_command(name))
}

fn collect_function_names(sources: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut functions = BTreeSet::new();
    for (path, source) in sources {
        let tree = SyntaxTree::parse(path, source);
        for line in tree.lines {
            if let SyntaxKind::Command(command) = line.kind
                && canonical_command(&command.name) == "func"
                && let Some(name) = function_name(&command)
            {
                functions.insert(name);
            }
        }
    }
    functions
}

fn split_inline_if(raw: &str) -> (&str, Option<&str>) {
    // Braced commands must be detected before keyword markers; otherwise the
    // opening brace becomes part of the condition (`if x { goto ... }`).
    if let Some(open) = raw.find('{')
        && let Some(close) = raw.rfind('}')
        && close > open
    {
        return (raw[..open].trim(), Some(raw[open + 1..close].trim()));
    }
    const COMMANDS: &[&str] = &[
        " goto ", " gosub ", " text ", " bg ", " lsp ", " csp ", " mov ", " let ", " add ",
        " sub ", " save ", " load ", " end ",
    ];
    let lowered = raw.to_ascii_lowercase();
    for marker in COMMANDS {
        if let Some(index) = lowered.find(marker) {
            let command_start = index + 1;
            return (
                raw[..index].trim().trim_end_matches('{').trim_end(),
                Some(raw[command_start..].trim().trim_end_matches('}').trim_end()),
            );
        }
    }
    (raw.trim(), None)
}

fn tokenize_expression(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in raw.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            current.push(character);
            escaped = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            current.push(character);
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

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
                // Package paths are canonical NFC rather than the host
                // filesystem's spelling. This prevents HFS/APFS and Linux
                // from silently producing different logical asset IDs.
                parts.push(other.nfc().collect::<String>());
            }
        }
    }
    if parts.is_empty() {
        return Err("logical path is empty".to_owned());
    }
    Ok(parts.join("/"))
}

/// Returns the comparison key used to reject paths that would collide on a
/// case-insensitive Windows filesystem. The packaged path itself remains
/// case-sensitive; this is an early portability diagnostic, not a lossy path
/// transform.
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

fn is_modern_source(source: &str) -> bool {
    crate::modern::parse("<language-detection>", source)
        .cst
        .tokens
        .iter()
        .find(|token| {
            !matches!(
                token.kind,
                crate::modern::ModernTokenKind::Whitespace
                    | crate::modern::ModernTokenKind::LineComment
                    | crate::modern::ModernTokenKind::BlockComment
                    | crate::modern::ModernTokenKind::Eof
            )
        })
        .is_some_and(|token| {
            token.kind == crate::modern::ModernTokenKind::Identifier && token.text == "aria"
        })
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
    fn compiles_vertical_slice_and_resolves_control_flow() {
        let output = compile_script(
            "# aria-version: 3.0\n\
             *start\n\
             let %route, 1\n\
             if %route == 1\n\
               bg \"assets/sea.webp\", 250\n\
             else\n\
               bg \"#000000\", 0\n\
             endif\n\
             ミオ「海へ行こう。」\n\
             choice \"行く\", *go, \"戻る\", *end\n\
             *go\n\
             dwave 0, \"assets/wave.ogg\"\n\
             *end\n\
             end\n",
        );
        assert!(!output.has_errors(), "{:?}", output.diagnostics);
        let program = output.program.unwrap();
        program.validate().unwrap();
        assert!(
            program
                .instructions
                .iter()
                .any(|value| value.op == ByteOp::PresentChoice)
        );
        assert!(
            program
                .instructions
                .iter()
                .any(|value| value.op == ByteOp::PlayAudio)
        );
    }

    #[test]
    fn line_dialogue_is_immediate_and_wait_is_only_explicit() {
        let output = compile_script(
            "# aria-version: 3.0\n\
             say \"一行目\"\n\
             ミオ「二行目」\n\
             @\n\
             end\n",
        );
        assert!(!output.has_errors(), "{:?}", output.diagnostics);
        let instructions = &output.program.unwrap().instructions;
        let text_positions = instructions
            .iter()
            .enumerate()
            .filter(|(_, instruction)| instruction.op == ByteOp::Text)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        assert_eq!(text_positions.len(), 2);
        assert_eq!(instructions[text_positions[0] + 1].op, ByteOp::Text);
        assert_eq!(instructions[text_positions[1] + 1].op, ByteOp::WaitAdvance);
    }

    #[test]
    fn rejects_legacy_language_at_runtime_boundary() {
        let output = compile_script("# aria-version: 2.0\nend\n");
        assert!(output.has_errors());
        assert!(
            output.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnsupportedLanguageVersion
            })
        );
    }

    #[test]
    fn includes_are_relative_and_compiled_once() {
        let output = compile(CompileInput {
            game_id: "jp.example.test".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![
                SourceUnit {
                    logical_path: "scripts/main.aria".to_owned(),
                    source: "# aria-version: 3.0\ninclude \"parts/a.aria\"\ninclude \"parts/a.aria\"\nend\n".to_owned(),
                },
                SourceUnit {
                    logical_path: "scripts/parts/a.aria".to_owned(),
                    source: "ミオ「一度だけ。」\n".to_owned(),
                },
            ],
        });
        assert!(!output.has_errors(), "{:?}", output.diagnostics);
        let text_count = output
            .program
            .unwrap()
            .instructions
            .iter()
            .filter(|instruction| instruction.op == ByteOp::Text)
            .count();
        assert_eq!(text_count, 1);
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
                    source: "# aria-version: 3.0\nend\n".to_owned(),
                },
                SourceUnit {
                    logical_path: "scripts/Main.aria".to_owned(),
                    source: "# aria-version: 3.0\nend\n".to_owned(),
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
