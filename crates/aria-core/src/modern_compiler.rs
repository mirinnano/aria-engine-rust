//! Semantic analysis and lowering for the structured Aria 3.1 language.
//!
//! This is deliberately separate from the alpha 3.0 line-command compiler.
//! The two front ends may coexist during migration, but both lower into the
//! same deterministic Core protocol and neither touches files, clocks, or
//! platform APIs.

use std::collections::{BTreeMap, BTreeSet};

use crate::bytecode::{
    ARIAC_FORMAT_VERSION, ByteOp, CompiledProgram, Constant, EncodedInstruction, LanguageVersion,
    Operand, SourceLocation,
};
use crate::compiler::{CompileOutput, normalize_logical_path, resolve_logical_path};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
use crate::modern::{
    AssetRef, AudioBus, BinaryOperator, Expression, ExpressionKind, Literal, ModernLanguageVersion,
    ModernModule, ModernType, ShowContent, Statement, StatementKind, TransitionKind, UnaryOperator,
    Value, parse,
};

pub(crate) fn compile_modern(
    game_id: String,
    entry: String,
    sources: BTreeMap<String, String>,
    diagnostics: Vec<Diagnostic>,
) -> CompileOutput {
    let mut compiler = ModernCompiler {
        game_id,
        entry: entry.clone(),
        sources,
        diagnostics,
        active_imports: Vec::new(),
        modules: BTreeMap::new(),
        source_order: Vec::new(),
        entry_scene: None,
        state_declarations: Vec::new(),
        scenes: Vec::new(),
        scene_names: BTreeSet::new(),
        state_bindings: BTreeMap::new(),
        constants: Vec::new(),
        string_constants: BTreeMap::new(),
        instructions: Vec::new(),
        source_map: Vec::new(),
        labels: BTreeMap::new(),
        references: Vec::new(),
        generated_label: 0,
        generated_storage: 0,
    };
    compiler.collect_module(&entry, true);
    compiler.lower()
}

#[derive(Debug, Clone)]
struct SceneSource {
    source: String,
    name: String,
    body: Vec<Statement>,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct StateSource {
    name: String,
    ty: ModernType,
    value: Literal,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct Binding {
    ty: ModernType,
    mutable: bool,
    operand: Operand,
}

#[derive(Debug, Clone)]
struct LabelReference {
    instruction: usize,
    operand: usize,
    label: String,
    span: SourceSpan,
}

/// A structured scene never falls through into the next declaration. This is
/// deliberately stricter than the V1/V2 label model: a scene's final control
/// transfer is part of its author-facing contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SceneExit {
    End,
    Return,
    Jump,
    Choice,
}

#[derive(Debug, Clone)]
struct CallSite {
    source_scene: String,
    target_scene: String,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct TransferSite {
    target_scene: String,
    span: SourceSpan,
}

struct ModernCompiler {
    game_id: String,
    entry: String,
    sources: BTreeMap<String, String>,
    diagnostics: Vec<Diagnostic>,
    active_imports: Vec<String>,
    modules: BTreeMap<String, ModernModule>,
    source_order: Vec<String>,
    entry_scene: Option<(String, SourceSpan)>,
    state_declarations: Vec<StateSource>,
    scenes: Vec<SceneSource>,
    scene_names: BTreeSet<String>,
    state_bindings: BTreeMap<String, Binding>,
    constants: Vec<Constant>,
    string_constants: BTreeMap<String, u32>,
    instructions: Vec<EncodedInstruction>,
    source_map: Vec<SourceLocation>,
    labels: BTreeMap<String, u32>,
    references: Vec<LabelReference>,
    generated_label: u64,
    generated_storage: u64,
}

impl ModernCompiler {
    fn collect_module(&mut self, path: &str, is_entry: bool) {
        if self.modules.contains_key(path) {
            return;
        }
        if self.active_imports.iter().any(|active| active == path) {
            self.error(
                DiagnosticCode::InvalidControlFlow,
                format!(
                    "import cycle: {} -> {path}",
                    self.active_imports.join(" -> ")
                ),
                None,
            );
            return;
        }
        let Some(source) = self.sources.get(path).cloned() else {
            self.error(
                DiagnosticCode::MissingSource,
                format!("imported source '{path}' is missing"),
                None,
            );
            return;
        };

        self.active_imports.push(path.to_owned());
        let parsed = parse(path, source);
        self.diagnostics.extend(parsed.diagnostics);
        let Some(module) = parsed.module else {
            self.error(
                DiagnosticCode::UnsupportedLanguageVersion,
                "Aria 3.1 source must begin with 'aria 3.1;'",
                Some(SourceSpan::line(path, 1, 1)),
            );
            self.active_imports.pop();
            return;
        };
        if module.language_version != ModernLanguageVersion::V3_1 {
            self.error(
                DiagnosticCode::UnsupportedLanguageVersion,
                format!(
                    "Aria 3.1 compiler accepts only 'aria 3.1;', found '{}.{}'",
                    module.language_version.major, module.language_version.minor
                ),
                Some(module.span.clone()),
            );
        }
        if is_entry {
            match &module.entry {
                Some(entry) => {
                    if self
                        .entry_scene
                        .replace((entry.scene.clone(), entry.span.clone()))
                        .is_some()
                    {
                        self.error(
                            DiagnosticCode::InvalidControlFlow,
                            "entry scene may be declared only once",
                            Some(entry.span.clone()),
                        );
                    }
                }
                None => self.error(
                    DiagnosticCode::InvalidControlFlow,
                    "entry module must declare 'entry <scene>;';",
                    Some(module.span.clone()),
                ),
            }
        } else if let Some(entry) = &module.entry {
            self.error(
                DiagnosticCode::InvalidControlFlow,
                "only the entry source may declare an entry scene",
                Some(entry.span.clone()),
            );
        }

        self.modules.insert(path.to_owned(), module.clone());
        for import in &module.imports {
            match resolve_logical_path(path, &import.path) {
                Ok(import_path) => self.collect_module(&import_path, false),
                Err(message) => self.error(
                    DiagnosticCode::MissingSource,
                    message,
                    Some(import.span.clone()),
                ),
            }
        }
        self.active_imports.pop();
        self.source_order.push(path.to_owned());
    }

    fn lower(mut self) -> CompileOutput {
        self.collect_declarations();
        self.lower_states();

        let entry_scene = self.entry_scene.clone();
        if let Some((scene, span)) = entry_scene {
            self.emit_label_reference(ByteOp::Jump, Vec::new(), &scene_label(&scene), &span);
        }

        let scenes = self.scenes.clone();
        for scene in scenes {
            self.define_label(&scene_label(&scene.name), &scene.span);
            let mut scopes = vec![self.state_bindings.clone(), BTreeMap::new()];
            self.lower_block(&scene.source, &scene.body, &mut scopes, &scene.name);
        }

        if self
            .instructions
            .last()
            .is_none_or(|instruction| instruction.op != ByteOp::End)
        {
            self.emit(
                ByteOp::End,
                Vec::new(),
                &SourceSpan::line(&self.entry, 1, 1),
            );
        }
        self.resolve_references();
        self.finish()
    }

    fn collect_declarations(&mut self) {
        let paths = self.source_order.clone();
        for path in paths {
            let Some(module) = self.modules.get(&path).cloned() else {
                continue;
            };
            for state in module.states {
                if self
                    .state_declarations
                    .iter()
                    .any(|existing| existing.name == state.name)
                {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!(
                            "duplicate saved state '{}'; state names are global",
                            state.name
                        ),
                        Some(state.span.clone()),
                    );
                } else {
                    self.state_declarations.push(StateSource {
                        name: state.name,
                        ty: state.ty,
                        value: state.value,
                        span: state.span,
                    });
                }
            }
            for scene in module.scenes {
                if !self.scene_names.insert(scene.name.clone()) {
                    self.error(
                        DiagnosticCode::DuplicateLabel,
                        format!("duplicate scene '{}'; scene names are global", scene.name),
                        Some(scene.span.clone()),
                    );
                } else {
                    self.scenes.push(SceneSource {
                        source: path.clone(),
                        name: scene.name,
                        body: scene.body,
                        span: scene.span,
                    });
                }
            }
        }

        if let Some((entry, span)) = &self.entry_scene
            && !self.scene_names.contains(entry)
        {
            self.error(
                DiagnosticCode::UnknownLabel,
                format!("entry scene '{entry}' is not declared"),
                Some(span.clone()),
            );
        }

        self.validate_scene_control_flow();
    }

    /// Ensures that lowering cannot accidentally create legacy label
    /// fallthrough or recursive calls with static local storage. The VM has a
    /// call stack, but Aria 3.1 deliberately does not expose recursive scenes
    /// until it has activation-local bindings; rejecting a cycle is safer and
    /// deterministic on every Player.
    fn validate_scene_control_flow(&mut self) {
        let scenes = self.scenes.clone();
        let mut contracts = BTreeMap::<String, BTreeSet<SceneExit>>::new();
        let mut calls = Vec::new();
        let mut transfers = Vec::new();

        for scene in &scenes {
            match self.block_exit_contract(&scene.body) {
                Some(exits) => {
                    contracts.insert(scene.name.clone(), exits);
                }
                None => self.error(
                    DiagnosticCode::InvalidControlFlow,
                    format!(
                        "scene '{}' can fall through; finish every path with end, return, jump, or choice",
                        scene.name
                    ),
                    Some(scene.span.clone()),
                ),
            }
            collect_scene_sites(&scene.body, &scene.name, &mut calls, &mut transfers);
        }

        if let Some((entry, span)) = &self.entry_scene
            && contracts
                .get(entry)
                .is_some_and(|exits| exits.contains(&SceneExit::Return))
        {
            self.error(
                DiagnosticCode::InvalidControlFlow,
                "an entry scene may not return; use end, jump, or choice",
                Some(span.clone()),
            );
        }

        for call in &calls {
            let Some(exits) = contracts.get(&call.target_scene) else {
                // Label resolution emits the authoritative unknown-scene
                // diagnostic with the same source span.
                continue;
            };
            if exits.len() != 1 || !exits.contains(&SceneExit::Return) {
                self.error(
                    DiagnosticCode::InvalidControlFlow,
                    format!(
                        "call target '{}' must return on every path",
                        call.target_scene
                    ),
                    Some(call.span.clone()),
                );
            }
        }

        for transfer in &transfers {
            let Some(exits) = contracts.get(&transfer.target_scene) else {
                continue;
            };
            if exits.contains(&SceneExit::Return) {
                self.error(
                    DiagnosticCode::InvalidControlFlow,
                    format!(
                        "jump/choice target '{}' may return without a caller; use end, jump, or choice instead",
                        transfer.target_scene
                    ),
                    Some(transfer.span.clone()),
                );
            }
        }

        let mut call_graph = BTreeMap::<String, BTreeSet<String>>::new();
        for scene in &scenes {
            call_graph.entry(scene.name.clone()).or_default();
        }
        for call in calls {
            if self.scene_names.contains(&call.target_scene) {
                call_graph
                    .entry(call.source_scene)
                    .or_default()
                    .insert(call.target_scene);
            }
        }
        if let Some(cycle) = find_call_cycle(&call_graph) {
            self.error(
                DiagnosticCode::InvalidControlFlow,
                format!(
                    "recursive scene calls are not supported in Aria 3.1: {}",
                    cycle.join(" -> ")
                ),
                None,
            );
        }
    }

    /// Returns the finite ways a block exits when every path is terminal.
    /// None means execution can continue past the block.
    fn block_exit_contract(&mut self, statements: &[Statement]) -> Option<BTreeSet<SceneExit>> {
        for (index, statement) in statements.iter().enumerate() {
            let Some(exits) = self.statement_exit_contract(statement) else {
                continue;
            };
            for unreachable in &statements[index + 1..] {
                self.error(
                    DiagnosticCode::InvalidControlFlow,
                    "statement is unreachable after a terminal control transfer",
                    Some(unreachable.span.clone()),
                );
            }
            return Some(exits);
        }
        None
    }

    fn statement_exit_contract(&mut self, statement: &Statement) -> Option<BTreeSet<SceneExit>> {
        let direct = match &statement.kind {
            StatementKind::End => Some(SceneExit::End),
            StatementKind::Return => Some(SceneExit::Return),
            StatementKind::Jump { .. } => Some(SceneExit::Jump),
            StatementKind::Choice { .. } => Some(SceneExit::Choice),
            _ => None,
        };
        if let Some(exit) = direct {
            return Some(BTreeSet::from([exit]));
        }
        let StatementKind::If {
            then_branch,
            else_branch,
            ..
        } = &statement.kind
        else {
            return None;
        };
        if else_branch.is_empty() {
            // An omitted else branch can always continue when the condition
            // is false, even if the then branch is terminal.
            let _ = self.block_exit_contract(then_branch);
            return None;
        }
        let then_exits = self.block_exit_contract(then_branch);
        let else_exits = self.block_exit_contract(else_branch);
        match (then_exits, else_exits) {
            (Some(mut then_exits), Some(else_exits)) => {
                then_exits.extend(else_exits);
                Some(then_exits)
            }
            _ => None,
        }
    }

    fn lower_states(&mut self) {
        let states = self.state_declarations.clone();
        for state in states {
            if state.ty != state.value.ty() {
                self.error(
                    DiagnosticCode::InvalidOperand,
                    format!(
                        "saved state '{}' is declared as {} but initialized with {}",
                        state.name,
                        type_name(state.ty),
                        type_name(state.value.ty())
                    ),
                    Some(state.value.span().clone()),
                );
                continue;
            }
            let operand = self.fresh_storage("state", &state.name, state.ty);
            let binding = Binding {
                ty: state.ty,
                mutable: true,
                operand: operand.clone(),
            };
            self.state_bindings.insert(state.name, binding);
            let value = self.literal_operand(&state.value);
            self.emit_assignment(&operand, state.ty, value, &state.span);
        }
    }

    fn lower_block(
        &mut self,
        source: &str,
        statements: &[Statement],
        scopes: &mut Vec<BTreeMap<String, Binding>>,
        scene: &str,
    ) {
        for statement in statements {
            self.lower_statement(source, statement, scopes, scene);
        }
    }

    fn lower_statement(
        &mut self,
        source: &str,
        statement: &Statement,
        scopes: &mut Vec<BTreeMap<String, Binding>>,
        scene: &str,
    ) {
        match &statement.kind {
            StatementKind::Say { speaker, text } => {
                let speaker = speaker
                    .as_ref()
                    .map(|speaker| Operand::Constant(self.intern_string(speaker)))
                    .unwrap_or(Operand::None);
                let text = Operand::Constant(self.intern_string(text));
                self.emit(ByteOp::Text, vec![speaker, text], &statement.span);
            }
            StatementKind::Narrate { text } => {
                let text = Operand::Constant(self.intern_string(text));
                self.emit(ByteOp::Text, vec![Operand::None, text], &statement.span);
            }
            StatementKind::ClearDialogue => {
                self.emit(ByteOp::TextClear, Vec::new(), &statement.span)
            }
            StatementKind::AwaitAdvance => self.emit(
                ByteOp::WaitAdvance,
                vec![Operand::Boolean(false)],
                &statement.span,
            ),
            StatementKind::Wait { duration_ms } => self.emit(
                ByteOp::Delay,
                vec![Operand::Integer(i64::from(*duration_ms))],
                &statement.span,
            ),
            StatementKind::Background { asset, transition } => {
                let Some(asset) = self.visual_asset_operand(asset) else {
                    return;
                };
                self.emit(
                    ByteOp::Background,
                    vec![asset, Operand::Integer(0)],
                    &statement.span,
                );
                if let Some(transition) = transition {
                    if transition.kind == TransitionKind::Mask {
                        self.error(
                            DiagnosticCode::InvalidOperand,
                            "mask transitions are not available in Aria 3.1 yet; use fade or wipe",
                            Some(transition.span.clone()),
                        );
                        return;
                    }
                    let kind = match transition.kind {
                        TransitionKind::Fade => "fade",
                        TransitionKind::Wipe => "wipe",
                        TransitionKind::Mask => unreachable!("handled above"),
                    };
                    let kind = Operand::Constant(self.intern_string(kind));
                    self.emit(
                        ByteOp::BeginTransition,
                        vec![
                            kind,
                            Operand::Integer(i64::from(transition.duration_ms.unwrap_or(300))),
                        ],
                        &transition.span,
                    );
                }
            }
            StatementKind::Show { id, content, z } => match content {
                ShowContent::Image { asset, position } => {
                    let Some(asset) = self.asset_operand(asset) else {
                        return;
                    };
                    let id = Operand::Constant(self.intern_string(id));
                    self.emit(
                        ByteOp::SpriteImage,
                        vec![
                            id,
                            asset,
                            Operand::Integer(i64::from(position.x_px)),
                            Operand::Integer(i64::from(position.y_px)),
                            Operand::Integer(i64::from(*z)),
                            Operand::Integer(255),
                        ],
                        &statement.span,
                    );
                }
                ShowContent::Rect { bounds, color } => {
                    if !valid_color(color) {
                        self.error(
                            DiagnosticCode::InvalidOperand,
                            "rect color must use #RGB, #RGBA, #RRGGBB, or #RRGGBBAA",
                            Some(statement.span.clone()),
                        );
                        return;
                    }
                    let id = Operand::Constant(self.intern_string(id));
                    let color = Operand::Constant(self.intern_string(color));
                    self.emit(
                        ByteOp::SpriteRect,
                        vec![
                            id,
                            Operand::Integer(i64::from(bounds.x_px)),
                            Operand::Integer(i64::from(bounds.y_px)),
                            Operand::Integer(i64::from(bounds.width_px)),
                            Operand::Integer(i64::from(bounds.height_px)),
                            color,
                            Operand::Integer(i64::from(*z)),
                        ],
                        &statement.span,
                    );
                }
                ShowContent::Text {
                    text,
                    position,
                    size_px,
                } => {
                    if *size_px <= 0 {
                        self.error(
                            DiagnosticCode::InvalidOperand,
                            "text size must be greater than zero",
                            Some(statement.span.clone()),
                        );
                        return;
                    }
                    let id = Operand::Constant(self.intern_string(id));
                    let text = Operand::Constant(self.intern_string(text));
                    self.emit(
                        ByteOp::SpriteText,
                        vec![
                            id,
                            text,
                            Operand::Integer(i64::from(position.x_px)),
                            Operand::Integer(i64::from(position.y_px)),
                            Operand::Integer(i64::from(*size_px)),
                            Operand::Integer(i64::from(*z)),
                        ],
                        &statement.span,
                    );
                }
            },
            StatementKind::Hide { id } => {
                let id = Operand::Constant(self.intern_string(id));
                self.emit(
                    ByteOp::SpriteVisibility,
                    vec![id, Operand::Boolean(false)],
                    &statement.span,
                );
            }
            StatementKind::Remove { id } => {
                let id = Operand::Constant(self.intern_string(id));
                self.emit(ByteOp::SpriteRemove, vec![id], &statement.span);
            }
            StatementKind::Move { id, position } => {
                let id = Operand::Constant(self.intern_string(id));
                self.emit(
                    ByteOp::SpriteMove,
                    vec![
                        id,
                        Operand::Integer(i64::from(position.x_px)),
                        Operand::Integer(i64::from(position.y_px)),
                    ],
                    &statement.span,
                );
            }
            StatementKind::Declare {
                mutable,
                name,
                ty,
                value,
            } => self.lower_declaration(value, *mutable, name, *ty, statement, scopes, scene),
            StatementKind::Assign { name, value } => {
                let Some(binding) = resolve_binding(scopes, name) else {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("unknown variable '{name}'"),
                        Some(statement.span.clone()),
                    );
                    return;
                };
                if !binding.mutable {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("cannot assign to immutable 'let' variable '{name}'"),
                        Some(statement.span.clone()),
                    );
                    return;
                }
                let value_span = value.span().clone();
                let Some((operand, value_ty)) = self.value_operand(value, scopes) else {
                    return;
                };
                if value_ty != binding.ty {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!(
                            "cannot assign {} to '{}' declared as {}",
                            type_name(value_ty),
                            name,
                            type_name(binding.ty)
                        ),
                        Some(value_span),
                    );
                    return;
                }
                self.emit_assignment(&binding.operand, binding.ty, operand, &statement.span);
            }
            StatementKind::AddAssign { name, value } => {
                let Some(binding) = resolve_binding(scopes, name) else {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("unknown variable '{name}'"),
                        Some(statement.span.clone()),
                    );
                    return;
                };
                if !binding.mutable || binding.ty != ModernType::Int {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("'{name} += …' requires a mutable Int variable"),
                        Some(statement.span.clone()),
                    );
                    return;
                }
                self.emit(
                    ByteOp::AddInt,
                    vec![
                        binding.operand,
                        Operand::Integer(*value),
                        Operand::Integer(1),
                    ],
                    &statement.span,
                );
            }
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let false_label = self.fresh_label("if_false");
                let end_label = self.fresh_label("if_end");
                self.emit_jump_if_false(condition, &false_label, scopes);
                self.with_scope(scopes, |compiler, scopes| {
                    compiler.lower_block(source, then_branch, scopes, scene);
                });
                if else_branch.is_empty() {
                    self.define_label(&false_label, &statement.span);
                } else {
                    self.emit_label_reference(
                        ByteOp::Jump,
                        Vec::new(),
                        &end_label,
                        &statement.span,
                    );
                    self.define_label(&false_label, &statement.span);
                    self.with_scope(scopes, |compiler, scopes| {
                        compiler.lower_block(source, else_branch, scopes, scene);
                    });
                    self.define_label(&end_label, &statement.span);
                }
            }
            StatementKind::While { condition, body } => {
                let start_label = self.fresh_label("while_start");
                let end_label = self.fresh_label("while_end");
                self.define_label(&start_label, &statement.span);
                self.emit_jump_if_false(condition, &end_label, scopes);
                self.with_scope(scopes, |compiler, scopes| {
                    compiler.lower_block(source, body, scopes, scene);
                });
                self.emit_label_reference(ByteOp::Jump, Vec::new(), &start_label, &statement.span);
                self.define_label(&end_label, &statement.span);
            }
            StatementKind::Choice { options } => {
                if options.is_empty() {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        "choice requires at least one option",
                        Some(statement.span.clone()),
                    );
                    return;
                }
                let mut operands = Vec::with_capacity(options.len() * 2);
                for option in options {
                    operands.push(Operand::Constant(self.intern_string(&option.text)));
                    let operand_index = operands.len();
                    operands.push(Operand::Address(0));
                    self.references.push(LabelReference {
                        instruction: self.instructions.len(),
                        operand: operand_index,
                        label: scene_label(&option.scene),
                        span: option.span.clone(),
                    });
                }
                self.emit(ByteOp::PresentChoice, operands, &statement.span);
            }
            StatementKind::Jump { scene } => {
                self.emit_label_reference(
                    ByteOp::Jump,
                    Vec::new(),
                    &scene_label(scene),
                    &statement.span,
                );
            }
            StatementKind::Call { scene } => {
                self.emit_label_reference(
                    ByteOp::Call,
                    Vec::new(),
                    &scene_label(scene),
                    &statement.span,
                );
            }
            StatementKind::Return => self.emit(ByteOp::Return, Vec::new(), &statement.span),
            StatementKind::Play {
                bus,
                asset,
                looping,
                fade_ms,
            } => {
                let Some(asset) = self.asset_operand(asset) else {
                    return;
                };
                let bus_name = audio_bus_name(*bus);
                let id = match bus {
                    AudioBus::Bgm => bus_name.to_owned(),
                    AudioBus::Se | AudioBus::Voice => {
                        asset_path_for_id(asset.clone(), &self.constants)
                    }
                };
                let bus = Operand::Constant(self.intern_string(bus_name));
                let id = Operand::Constant(self.intern_string(&id));
                self.emit(
                    ByteOp::PlayAudio,
                    vec![
                        bus,
                        id,
                        asset,
                        Operand::Boolean(*looping),
                        Operand::Float(1.0),
                        Operand::Integer(i64::from(fade_ms.unwrap_or(0))),
                    ],
                    &statement.span,
                );
            }
            StatementKind::Stop { bus, fade_ms } => {
                let bus = Operand::Constant(self.intern_string(audio_bus_name(*bus)));
                self.emit(
                    ByteOp::StopAudio,
                    vec![
                        bus,
                        Operand::None,
                        Operand::Integer(i64::from(fade_ms.unwrap_or(0))),
                    ],
                    &statement.span,
                );
            }
            StatementKind::Volume { bus, value } => {
                if !(0.0..=1.0).contains(value) {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        "volume must be between 0.0 and 1.0",
                        Some(statement.span.clone()),
                    );
                    return;
                }
                let bus = Operand::Constant(self.intern_string(audio_bus_name(*bus)));
                self.emit(
                    ByteOp::SetVolume,
                    vec![bus, Operand::Float(*value as f32), Operand::Integer(0)],
                    &statement.span,
                );
            }
            StatementKind::Save { slot } => self.emit(
                ByteOp::Save,
                vec![Operand::Integer(i64::from(*slot))],
                &statement.span,
            ),
            StatementKind::Load { slot } => self.emit(
                ByteOp::Load,
                vec![Operand::Integer(i64::from(*slot))],
                &statement.span,
            ),
            StatementKind::End => self.emit(ByteOp::End, Vec::new(), &statement.span),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_declaration(
        &mut self,
        value: &Value,
        mutable: bool,
        name: &str,
        ty: ModernType,
        statement: &Statement,
        scopes: &mut [BTreeMap<String, Binding>],
        scene: &str,
    ) {
        let Some(scope) = scopes.last() else {
            self.error(
                DiagnosticCode::InvalidControlFlow,
                "internal compiler scope underflow",
                Some(statement.span.clone()),
            );
            return;
        };
        if scope.contains_key(name) {
            self.error(
                DiagnosticCode::InvalidOperand,
                format!("duplicate variable '{name}' in this scope"),
                Some(statement.span.clone()),
            );
            return;
        }
        let Some((value, value_ty)) = self.value_operand(value, scopes) else {
            return;
        };
        if value_ty != ty {
            self.error(
                DiagnosticCode::InvalidOperand,
                format!(
                    "variable '{name}' is declared as {} but initialized with {}",
                    type_name(ty),
                    type_name(value_ty)
                ),
                Some(statement.span.clone()),
            );
            return;
        }
        let operand = self.fresh_storage(scene, name, ty);
        self.emit_assignment(&operand, ty, value, &statement.span);
        if let Some(scope) = scopes.last_mut() {
            scope.insert(
                name.to_owned(),
                Binding {
                    ty,
                    mutable,
                    operand,
                },
            );
        }
    }

    fn with_scope(
        &mut self,
        scopes: &mut Vec<BTreeMap<String, Binding>>,
        action: impl FnOnce(&mut Self, &mut Vec<BTreeMap<String, Binding>>),
    ) {
        scopes.push(BTreeMap::new());
        action(self, scopes);
        let _ = scopes.pop();
    }

    fn emit_jump_if_false(
        &mut self,
        expression: &Expression,
        false_label: &str,
        scopes: &[BTreeMap<String, Binding>],
    ) {
        match &expression.kind {
            ExpressionKind::Unary {
                op: UnaryOperator::Not,
                expression,
            } => self.emit_jump_if_true(expression, false_label, scopes),
            ExpressionKind::Binary {
                left,
                op: BinaryOperator::And,
                right,
            } => {
                self.emit_jump_if_false(left, false_label, scopes);
                self.emit_jump_if_false(right, false_label, scopes);
            }
            ExpressionKind::Binary {
                left,
                op: BinaryOperator::Or,
                right,
            } => {
                let evaluate_right = self.fresh_label("or_right");
                let passed = self.fresh_label("or_passed");
                self.emit_jump_if_false(left, &evaluate_right, scopes);
                self.emit_label_reference(ByteOp::Jump, Vec::new(), &passed, &expression.span);
                self.define_label(&evaluate_right, &expression.span);
                self.emit_jump_if_false(right, false_label, scopes);
                self.define_label(&passed, &expression.span);
            }
            _ => self.emit_simple_condition(expression, false_label, scopes),
        }
    }

    fn emit_jump_if_true(
        &mut self,
        expression: &Expression,
        true_label: &str,
        scopes: &[BTreeMap<String, Binding>],
    ) {
        let not_true = self.fresh_label("condition_not_true");
        self.emit_jump_if_false(expression, &not_true, scopes);
        self.emit_label_reference(ByteOp::Jump, Vec::new(), true_label, &expression.span);
        self.define_label(&not_true, &expression.span);
    }

    fn emit_simple_condition(
        &mut self,
        expression: &Expression,
        false_label: &str,
        scopes: &[BTreeMap<String, Binding>],
    ) {
        let (left, comparator, right) = match &expression.kind {
            ExpressionKind::Literal(Literal::Boolean { value, .. }) => {
                (Operand::Boolean(*value), "==", Operand::Boolean(true))
            }
            ExpressionKind::Identifier(name) => {
                let Some(binding) = resolve_binding(scopes, name) else {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("unknown variable '{name}'"),
                        Some(expression.span.clone()),
                    );
                    return;
                };
                if binding.ty != ModernType::Bool {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        "if and while conditions must be Bool expressions",
                        Some(expression.span.clone()),
                    );
                    return;
                }
                (binding.operand, "==", Operand::Boolean(true))
            }
            ExpressionKind::Binary { left, op, right }
                if matches!(
                    op,
                    BinaryOperator::Equal
                        | BinaryOperator::NotEqual
                        | BinaryOperator::Less
                        | BinaryOperator::LessEqual
                        | BinaryOperator::Greater
                        | BinaryOperator::GreaterEqual
                ) =>
            {
                let Some((left_operand, left_ty)) = self.expression_value(left, scopes) else {
                    return;
                };
                let Some((right_operand, right_ty)) = self.expression_value(right, scopes) else {
                    return;
                };
                if left_ty != right_ty {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!(
                            "comparison operands must have the same type, found {} and {}",
                            type_name(left_ty),
                            type_name(right_ty)
                        ),
                        Some(expression.span.clone()),
                    );
                    return;
                }
                if !matches!(op, BinaryOperator::Equal | BinaryOperator::NotEqual)
                    && left_ty != ModernType::Int
                {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        "ordering comparisons require Int operands",
                        Some(expression.span.clone()),
                    );
                    return;
                }
                (left_operand, comparator_name(*op), right_operand)
            }
            _ => {
                self.error(
                    DiagnosticCode::InvalidOperand,
                    "if and while conditions must be Bool expressions",
                    Some(expression.span.clone()),
                );
                return;
            }
        };
        let comparator = Operand::Constant(self.intern_string(comparator));
        self.emit_label_reference(
            ByteOp::JumpIfFalse,
            vec![left, comparator, right],
            false_label,
            &expression.span,
        );
    }

    fn expression_value(
        &mut self,
        expression: &Expression,
        scopes: &[BTreeMap<String, Binding>],
    ) -> Option<(Operand, ModernType)> {
        match &expression.kind {
            ExpressionKind::Literal(literal) => Some((self.literal_operand(literal), literal.ty())),
            ExpressionKind::Identifier(name) => resolve_binding(scopes, name)
                .map(|binding| (binding.operand, binding.ty))
                .or_else(|| {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("unknown variable '{name}'"),
                        Some(expression.span.clone()),
                    );
                    None
                }),
            _ => {
                self.error(
                    DiagnosticCode::InvalidOperand,
                    "comparison operands must be a literal or variable",
                    Some(expression.span.clone()),
                );
                None
            }
        }
    }

    fn value_operand(
        &mut self,
        value: &Value,
        scopes: &[BTreeMap<String, Binding>],
    ) -> Option<(Operand, ModernType)> {
        match value {
            Value::Literal(literal) => Some((self.literal_operand(literal), literal.ty())),
            Value::Identifier { span, name } => resolve_binding(scopes, name)
                .map(|binding| (binding.operand, binding.ty))
                .or_else(|| {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("unknown variable '{name}'"),
                        Some(span.clone()),
                    );
                    None
                }),
        }
    }

    fn literal_operand(&mut self, literal: &Literal) -> Operand {
        match literal {
            Literal::Integer { value, .. } => Operand::Integer(*value),
            Literal::Boolean { value, .. } => Operand::Boolean(*value),
            Literal::String { value, .. } => Operand::Constant(self.intern_string(value)),
        }
    }

    fn emit_assignment(
        &mut self,
        target: &Operand,
        ty: ModernType,
        value: Operand,
        span: &SourceSpan,
    ) {
        let op = match ty {
            ModernType::Int | ModernType::Bool => ByteOp::SetInt,
            ModernType::String => ByteOp::SetString,
        };
        self.emit(op, vec![target.clone(), value], span);
    }

    fn asset_operand(&mut self, asset: &AssetRef) -> Option<Operand> {
        let path = self.canonical_asset_path(asset)?;
        Some(Operand::Constant(self.intern_string(&path)))
    }

    fn visual_asset_operand(&mut self, asset: &AssetRef) -> Option<Operand> {
        if asset.path.starts_with('#') {
            if !valid_color(&asset.path) {
                self.error(
                    DiagnosticCode::InvalidOperand,
                    "background color must use #RGB, #RGBA, #RRGGBB, or #RRGGBBAA",
                    Some(asset.span.clone()),
                );
                return None;
            }
            return Some(Operand::Constant(self.intern_string(&asset.path)));
        }
        self.asset_operand(asset)
    }

    fn canonical_asset_path(&mut self, asset: &AssetRef) -> Option<String> {
        if asset.path.starts_with('#') {
            self.error(
                DiagnosticCode::InvalidOperand,
                "this command requires an asset path, not a color",
                Some(asset.span.clone()),
            );
            return None;
        }
        let normalized = match normalize_logical_path(&asset.path) {
            Ok(path) => path,
            Err(message) => {
                self.error(
                    DiagnosticCode::InvalidOperand,
                    message,
                    Some(asset.span.clone()),
                );
                return None;
            }
        };
        if normalized != asset.path || asset.path.contains('\\') {
            self.error(
                DiagnosticCode::InvalidOperand,
                format!(
                    "asset path '{}' must already be a canonical project-relative '/' path",
                    asset.path
                ),
                Some(asset.span.clone()),
            );
            return None;
        }
        Some(normalized)
    }

    fn fresh_storage(&mut self, scope: &str, name: &str, ty: ModernType) -> Operand {
        let index = self.generated_storage;
        self.generated_storage = self.generated_storage.saturating_add(1);
        let key = format!("v3:{scope}:{name}:{index}");
        match ty {
            ModernType::Int | ModernType::Bool => Operand::IntRegister(key),
            ModernType::String => Operand::StringRegister(key),
        }
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
            label: label.to_owned(),
            span: span.clone(),
        });
        self.emit(op, operands, span);
    }

    fn define_label(&mut self, label: &str, span: &SourceSpan) {
        let address = u32::try_from(self.instructions.len()).unwrap_or(u32::MAX);
        if self.labels.insert(label.to_owned(), address).is_some() {
            self.error(
                DiagnosticCode::DuplicateLabel,
                format!("duplicate generated scene/control-flow label '{label}'"),
                Some(span.clone()),
            );
        }
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let label = format!("__aria31_{prefix}_{}", self.generated_label);
        self.generated_label = self.generated_label.saturating_add(1);
        label
    }

    fn resolve_references(&mut self) {
        let references = self.references.clone();
        for reference in &references {
            let Some(address) = self.labels.get(&reference.label).copied() else {
                let scene = reference
                    .label
                    .strip_prefix("scene:")
                    .unwrap_or(&reference.label);
                self.error(
                    DiagnosticCode::UnknownLabel,
                    format!("unknown scene or control-flow target '{scene}'"),
                    Some(reference.span.clone()),
                );
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
    }

    fn error(
        &mut self,
        code: DiagnosticCode,
        message: impl Into<String>,
        span: Option<SourceSpan>,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, message, span));
    }

    fn finish(mut self) -> CompileOutput {
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
        CompileOutput {
            program: (!has_errors).then_some(CompiledProgram {
                format_version: ARIAC_FORMAT_VERSION,
                language_version: LanguageVersion::V3_1,
                game_id: self.game_id,
                constants: self.constants,
                instructions: self.instructions,
                source_map: self.source_map,
            }),
            diagnostics: self.diagnostics,
        }
    }
}

fn collect_scene_sites(
    statements: &[Statement],
    source_scene: &str,
    calls: &mut Vec<CallSite>,
    transfers: &mut Vec<TransferSite>,
) {
    for statement in statements {
        match &statement.kind {
            StatementKind::Call { scene } => calls.push(CallSite {
                source_scene: source_scene.to_owned(),
                target_scene: scene.clone(),
                span: statement.span.clone(),
            }),
            StatementKind::Jump { scene } => transfers.push(TransferSite {
                target_scene: scene.clone(),
                span: statement.span.clone(),
            }),
            StatementKind::Choice { options } => {
                transfers.extend(options.iter().map(|option| TransferSite {
                    target_scene: option.scene.clone(),
                    span: option.span.clone(),
                }));
            }
            StatementKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_scene_sites(then_branch, source_scene, calls, transfers);
                collect_scene_sites(else_branch, source_scene, calls, transfers);
            }
            StatementKind::While { body, .. } => {
                collect_scene_sites(body, source_scene, calls, transfers);
            }
            StatementKind::Say { .. }
            | StatementKind::Narrate { .. }
            | StatementKind::ClearDialogue
            | StatementKind::AwaitAdvance
            | StatementKind::Wait { .. }
            | StatementKind::Background { .. }
            | StatementKind::Show { .. }
            | StatementKind::Hide { .. }
            | StatementKind::Remove { .. }
            | StatementKind::Move { .. }
            | StatementKind::Declare { .. }
            | StatementKind::Assign { .. }
            | StatementKind::AddAssign { .. }
            | StatementKind::Return
            | StatementKind::Play { .. }
            | StatementKind::Stop { .. }
            | StatementKind::Volume { .. }
            | StatementKind::Save { .. }
            | StatementKind::Load { .. }
            | StatementKind::End => {}
        }
    }
}

fn find_call_cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    let mut completed = BTreeSet::new();
    let mut active = Vec::new();
    let mut active_set = BTreeSet::new();
    for scene in graph.keys() {
        if let Some(cycle) =
            find_call_cycle_from(scene, graph, &mut completed, &mut active, &mut active_set)
        {
            return Some(cycle);
        }
    }
    None
}

fn find_call_cycle_from(
    scene: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    completed: &mut BTreeSet<String>,
    active: &mut Vec<String>,
    active_set: &mut BTreeSet<String>,
) -> Option<Vec<String>> {
    if completed.contains(scene) {
        return None;
    }
    if active_set.contains(scene) {
        let start = active.iter().position(|value| value == scene).unwrap_or(0);
        let mut cycle = active[start..].to_vec();
        cycle.push(scene.to_owned());
        return Some(cycle);
    }
    active.push(scene.to_owned());
    active_set.insert(scene.to_owned());
    if let Some(targets) = graph.get(scene) {
        for target in targets {
            if let Some(cycle) = find_call_cycle_from(target, graph, completed, active, active_set)
            {
                return Some(cycle);
            }
        }
    }
    active.pop();
    active_set.remove(scene);
    completed.insert(scene.to_owned());
    None
}

fn resolve_binding(scopes: &[BTreeMap<String, Binding>], name: &str) -> Option<Binding> {
    scopes
        .iter()
        .rev()
        .find_map(|scope| scope.get(name).cloned())
}

fn scene_label(name: &str) -> String {
    format!("scene:{name}")
}

fn type_name(ty: ModernType) -> &'static str {
    match ty {
        ModernType::Int => "Int",
        ModernType::Bool => "Bool",
        ModernType::String => "String",
    }
}

fn comparator_name(operator: BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Equal => "==",
        BinaryOperator::NotEqual => "!=",
        BinaryOperator::Less => "<",
        BinaryOperator::LessEqual => "<=",
        BinaryOperator::Greater => ">",
        BinaryOperator::GreaterEqual => ">=",
        BinaryOperator::And | BinaryOperator::Or => {
            unreachable!("handled before comparison lowering")
        }
    }
}

fn audio_bus_name(bus: AudioBus) -> &'static str {
    match bus {
        AudioBus::Bgm => "bgm",
        AudioBus::Se => "sound_effect",
        AudioBus::Voice => "voice",
    }
}

fn asset_path_for_id(operand: Operand, constants: &[Constant]) -> String {
    match operand {
        Operand::Constant(index) => constants
            .get(index as usize)
            .and_then(|constant| match constant {
                Constant::String(value) => Some(value.clone()),
                Constant::Integer(_) | Constant::Float(_) => None,
            })
            .unwrap_or_else(|| "audio".to_owned()),
        _ => "audio".to_owned(),
    }
}

fn valid_color(value: &str) -> bool {
    let Some(value) = value.strip_prefix('#') else {
        return false;
    };
    matches!(value.len(), 3 | 4 | 6 | 8) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{CompileInput, SourceUnit, compile};

    fn compile_script(source: &str) -> CompileOutput {
        compile(CompileInput {
            game_id: "jp.example.modern".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![SourceUnit {
                logical_path: "scripts/main.aria".to_owned(),
                source: source.to_owned(),
            }],
        })
    }

    #[test]
    fn structured_source_type_checks_and_lowers_without_host_opcodes() {
        let output = compile_script(
            "aria 3.1;\n\
             entry start;\n\
             state route: Int = 0;\n\
             scene start {\n\
               background asset(\"#07131f\") with fade(200ms);\n\
               show ミオ = image(asset(\"assets/mio.webp\")) at (760px, 86px) z 20;\n\
               say ミオ: \"海へ行こう。\";\n\
               choice { \"海\" => sea; \"駅\" => station; }\n\
             }\n\
             scene sea { var visits: Int = 0; visits += 1; if visits > 0 { play bgm asset(\"assets/sea.ogg\") loop; } end; }\n\
             scene station { end; }\n",
        );
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
        let program = output.program.unwrap();
        assert_eq!(program.language_version, LanguageVersion::V3_1);
        assert!(
            program
                .instructions
                .iter()
                .all(|instruction| instruction.op != ByteOp::Host)
        );
        program.validate().unwrap();
    }

    #[test]
    fn modern_semantics_reject_implicit_conversions_and_mutating_let() {
        let output = compile_script(
            "aria 3.1;\nentry start;\nscene start { let name: String = \"ミオ\"; name = \"別名\"; if name { end; } }\n",
        );
        assert!(output.has_errors());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("immutable"))
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Bool"))
        );
    }

    #[test]
    fn modern_dialogue_waits_only_when_authored_explicitly() {
        let output = compile_script(
            "aria 3.1;\nentry start;\nscene start { say \"海風\"; await advance; end; }\n",
        );
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
        let instructions = &output.program.unwrap().instructions;
        let text = instructions
            .iter()
            .position(|instruction| instruction.op == ByteOp::Text)
            .unwrap();
        assert_eq!(instructions[text + 1].op, ByteOp::WaitAdvance);
    }

    #[test]
    fn scenes_cannot_fall_through_and_imports_are_library_sources() {
        let output = compile(CompileInput {
            game_id: "jp.example.modern".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![
                SourceUnit {
                    logical_path: "scripts/main.aria".to_owned(),
                    source: "aria 3.1;\nimport \"./common.aria\";\nentry start;\nscene start { call helper; end; }\n".to_owned(),
                },
                SourceUnit {
                    logical_path: "scripts/common.aria".to_owned(),
                    source: "aria 3.1;\nscene helper { return; }\n".to_owned(),
                },
            ],
        });
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);

        let fallthrough = compile_script(
            "aria 3.1;\nentry start;\nscene start { narrate \"missing terminator\"; }\n",
        );
        assert!(fallthrough.has_errors());
        assert!(fallthrough.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("scene 'start' can fall through")
        }));
    }

    #[test]
    fn recursive_calls_and_jumps_to_returning_scenes_are_rejected() {
        let output = compile_script(
            "aria 3.1;\n+             entry start;\n+             scene start { call helper; end; }\n+             scene helper { call helper; return; }\n",
        );
        assert!(output.has_errors());
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("recursive scene calls are not supported")
        }));

        let jump_to_return = compile_script(
            "aria 3.1;\nentry start;\nscene start { jump helper; }\nscene helper { return; }\n",
        );
        assert!(jump_to_return.has_errors());
        assert!(
            jump_to_return
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("may return without a caller") })
        );
    }
}
