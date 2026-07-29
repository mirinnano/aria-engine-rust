//! Semantic analysis and lowering for the single, ownership-aware Aria
//! language. Source has no compatibility mode or language-version switch:
//! every project is parsed, checked, and lowered by this front end.

use std::collections::{BTreeMap, BTreeSet};

use crate::bytecode::{
    ARIAC_FORMAT_VERSION, ByteOp, CompiledProgram, Constant, EncodedInstruction, LanguageVersion,
    Operand, SourceLocation,
};
use crate::compiler::{CompileOutput, normalize_logical_path, resolve_logical_path};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
use crate::modern::{
    AssetRef, AudioBus, BinaryOperator, Expression, ExpressionKind, Literal, ModernModule,
    ModernType, NodeAccess, ShowContent, Statement, StatementKind, TransitionKind, UnaryOperator,
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
        generated_resource: 0,
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
    mutable: bool,
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
    resource: Option<ResourceBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResourceBinding {
    id: String,
    drop_order: u64,
    ownership: ResourceOwnership,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceOwnership {
    /// The lexical binding owns the resource and must release it exactly once.
    Owned,
    /// `drop` or an ownership transfer consumed this binding.
    Moved,
    /// An alias owns no resource and can only use the permissions granted by
    /// the enclosing `borrow` block.
    Borrowed { mutable: bool },
    /// The owner is unavailable until its borrow scope is closed.
    Loaned { mutable: bool },
}

#[derive(Debug, Clone)]
struct LabelReference {
    instruction: usize,
    operand: usize,
    label: String,
    span: SourceSpan,
}

/// A structured scene never falls through into the next declaration. A
/// scene's final control transfer is part of its author-facing contract.
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
    generated_resource: u64,
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
                "Aria source must begin with 'aria;'",
                Some(SourceSpan::line(path, 1, 1)),
            );
            self.active_imports.pop();
            return;
        };
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
        self.reject_visual_ui_declarations();
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
                        mutable: state.mutable,
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

    fn reject_visual_ui_declarations(&mut self) {
        for path in self.source_order.clone() {
            let declarations = self
                .modules
                .get(&path)
                .map(|module| {
                    module
                        .retired_ui_syntax
                        .iter()
                        .map(|declaration| declaration.span.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for span in declarations {
                self.error(
                    DiagnosticCode::DeprecatedUiSyntax,
                    "visual UI declarations are retired; move layout and styling to the project's React presentation package",
                    Some(span),
                );
            }
        }
    }

    /// Ensures that lowering cannot accidentally create label fallthrough or
    /// recursive calls with static local storage. The VM has a call stack, but
    /// Aria does not expose recursive scenes until it has activation-local
    /// bindings; rejecting a cycle is safer and deterministic on every
    /// Player.
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
                    "recursive scene calls are not supported: {}",
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
            return match &statement.kind {
                // A borrow block is lexical syntax, not a control-flow
                // boundary. A terminal transfer inside it also terminates the
                // containing scene path.
                StatementKind::Borrow { body, .. } => self.block_exit_contract(body),
                _ => None,
            };
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
                mutable: state.mutable,
                operand: operand.clone(),
                resource: None,
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
            StatementKind::Wait {
                duration_ms,
                release_after_ms,
            } => {
                let mut operands = vec![Operand::Integer(i64::from(*duration_ms))];
                if let Some(release_after_ms) = release_after_ms {
                    operands.push(Operand::Integer(i64::from(*release_after_ms)));
                }
                self.emit(ByteOp::Delay, operands, &statement.span);
            }
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
                            "mask transitions are not available yet; use fade, fade_through_black, or wipe",
                            Some(transition.span.clone()),
                        );
                        return;
                    }
                    let kind = match transition.kind {
                        TransitionKind::Fade => "fade",
                        TransitionKind::FadeThroughBlack => "fade_through_black",
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
            StatementKind::Spawn {
                mutable,
                name,
                content,
                z,
            } => self.lower_spawn(*mutable, name, content, *z, statement, scopes, scene),
            StatementKind::Hide { node } => {
                let Some(id) = self.node_mut_borrow(node, scopes) else {
                    return;
                };
                let id = Operand::Constant(self.intern_string(&id));
                self.emit(
                    ByteOp::SpriteVisibility,
                    vec![id, Operand::Boolean(false)],
                    &statement.span,
                );
            }
            StatementKind::Reveal { node } => {
                let Some(id) = self.node_mut_borrow(node, scopes) else {
                    return;
                };
                let id = Operand::Constant(self.intern_string(&id));
                self.emit(
                    ByteOp::SpriteVisibility,
                    vec![id, Operand::Boolean(true)],
                    &statement.span,
                );
            }
            StatementKind::Drop { name } => {
                let Some(id) = self.consume_owned_node(name, scopes, &statement.span) else {
                    return;
                };
                let id = Operand::Constant(self.intern_string(&id));
                self.emit(ByteOp::SpriteRemove, vec![id], &statement.span);
            }
            StatementKind::Move { node, position } => {
                let Some(id) = self.node_mut_borrow(node, scopes) else {
                    return;
                };
                let id = Operand::Constant(self.intern_string(&id));
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
                if binding.resource.is_some() {
                    self.error(
                        DiagnosticCode::InvalidOwnership,
                        format!(
                            "cannot assign a Node binding '{name}'; move it into a new 'let' binding or drop it"
                        ),
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
                // Compile both paths against the same incoming ownership
                // state. Sharing `scopes` here would incorrectly make a
                // consume in the first emitted branch affect the other
                // branch, despite those paths being mutually exclusive at
                // runtime.
                let before = scopes.clone();
                let mut then_scopes = before.clone();
                let mut else_scopes = before.clone();
                let then_continues = !block_definitely_exits(then_branch);
                let else_continues = else_branch.is_empty() || !block_definitely_exits(else_branch);
                self.emit_jump_if_false(condition, &false_label, scopes);
                self.with_scope(&mut then_scopes, &statement.span, |compiler, scopes| {
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
                    self.with_scope(&mut else_scopes, &statement.span, |compiler, scopes| {
                        compiler.lower_block(source, else_branch, scopes, scene);
                    });
                    self.define_label(&end_label, &statement.span);
                }

                *scopes = match (then_continues, else_continues) {
                    (true, true) => self.merge_continuing_ownership(
                        &before,
                        &then_scopes,
                        &else_scopes,
                        &statement.span,
                        "conditional",
                    ),
                    (true, false) => then_scopes,
                    (false, true) => else_scopes,
                    // Neither path reaches the continuation. The control-flow
                    // validator reports following statements as unreachable;
                    // retaining the incoming model avoids cascading ownership
                    // diagnostics while it does so.
                    (false, false) => before,
                };
            }
            StatementKind::While { condition, body } => {
                let start_label = self.fresh_label("while_start");
                let end_label = self.fresh_label("while_end");
                // A loop may execute zero times, so a resource that is still
                // reachable after it must have exactly the same ownership
                // state before and after every continuing iteration.
                let before = scopes.clone();
                let mut body_scopes = before.clone();
                let body_continues = !block_definitely_exits(body);
                self.define_label(&start_label, &statement.span);
                self.emit_jump_if_false(condition, &end_label, scopes);
                self.with_scope(&mut body_scopes, &statement.span, |compiler, scopes| {
                    compiler.lower_block(source, body, scopes, scene);
                });
                self.emit_label_reference(ByteOp::Jump, Vec::new(), &start_label, &statement.span);
                self.define_label(&end_label, &statement.span);
                if body_continues {
                    let _ = self.merge_continuing_ownership(
                        &before,
                        &body_scopes,
                        &before,
                        &statement.span,
                        "loop",
                    );
                }
                *scopes = before;
            }
            StatementKind::Borrow {
                mutable,
                owner,
                alias,
                body,
            } => self.lower_borrow(
                *mutable, owner, alias, body, statement, scopes, scene, source,
            ),
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
                self.cleanup_all_scopes(scopes, &statement.span);
                self.emit(ByteOp::PresentChoice, operands, &statement.span);
            }
            StatementKind::Jump { scene } => {
                self.cleanup_all_scopes(scopes, &statement.span);
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
            StatementKind::Return => {
                self.cleanup_all_scopes(scopes, &statement.span);
                self.emit(ByteOp::Return, Vec::new(), &statement.span);
            }
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
            StatementKind::SetFlag {
                name,
                value,
                persistent,
            } => {
                let op = if *persistent {
                    ByteOp::SetPersistentFlag
                } else {
                    ByteOp::SetFlag
                };
                let name = Operand::Constant(self.intern_string(name));
                self.emit(op, vec![name, Operand::Boolean(*value)], &statement.span);
            }
            StatementKind::SetTextSpeed { speed_ms } => self.emit(
                ByteOp::SetTextSpeed,
                vec![Operand::Integer(i64::from(*speed_ms))],
                &statement.span,
            ),
            StatementKind::SetAuto { enabled } => self.emit(
                ByteOp::SetAutoMode,
                vec![Operand::Boolean(*enabled)],
                &statement.span,
            ),
            StatementKind::SetSkip { mode } => {
                let mode = Operand::Constant(self.intern_string(mode));
                self.emit(ByteOp::SetSkipMode, vec![mode], &statement.span);
            }
            StatementKind::SetLocale { locale } => {
                let locale = Operand::Constant(self.intern_string(locale));
                self.emit(ByteOp::SetLocale, vec![locale], &statement.span);
            }
            StatementKind::SetTheme { theme } => {
                let _ = theme;
                self.error(
                    DiagnosticCode::DeprecatedUiSyntax,
                    "'theme' is retired; define visual tokens in the project's React presentation package",
                    Some(statement.span.clone()),
                );
            }
            StatementKind::SetTextBox {
                bounds,
                color,
                opacity,
                mode,
            } => {
                let _ = (bounds, color, opacity, mode);
                self.error(
                    DiagnosticCode::DeprecatedUiSyntax,
                    "'textbox' is retired; render dialogue in the project's React presentation package",
                    Some(statement.span.clone()),
                );
            }
            StatementKind::Tween {
                node,
                property,
                value,
                duration_ms,
                easing,
            } => {
                let Some(id) = self.node_mut_borrow(node, scopes) else {
                    return;
                };
                let id = Operand::Constant(self.intern_string(&id));
                let property = Operand::Constant(self.intern_string(property));
                let easing = Operand::Constant(self.intern_string(easing));
                self.emit(
                    ByteOp::TweenSprite,
                    vec![
                        id,
                        property,
                        Operand::Float(*value as f32),
                        Operand::Integer(i64::from(*duration_ms)),
                        easing,
                    ],
                    &statement.span,
                );
            }
            StatementKind::Effect {
                kind,
                color,
                amount,
                duration_ms,
                axis,
            } => {
                if !valid_color(color) {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        "effect color must use #RGB, #RGBA, #RRGGBB, or #RRGGBBAA",
                        Some(statement.span.clone()),
                    );
                } else {
                    let kind = Operand::Constant(self.intern_string(kind));
                    let color = Operand::Constant(self.intern_string(color));
                    let axis = Operand::Constant(self.intern_string(axis));
                    self.emit(
                        ByteOp::ScreenEffect,
                        vec![
                            kind,
                            color,
                            Operand::Float(*amount as f32),
                            Operand::Integer(i64::from(*duration_ms)),
                            axis,
                        ],
                        &statement.span,
                    );
                }
            }
            StatementKind::UnlockChapter { id, progress } => {
                let id = Operand::Constant(self.intern_string(id));
                self.emit(
                    ByteOp::UnlockChapter,
                    vec![id, Operand::Integer(i64::from(*progress))],
                    &statement.span,
                );
            }
            StatementKind::SetChapterProgress { id, progress } => {
                let id = Operand::Constant(self.intern_string(id));
                self.emit(
                    ByteOp::SetChapterProgress,
                    vec![id, Operand::Integer(i64::from(*progress))],
                    &statement.span,
                );
            }
            StatementKind::UnlockCg { id } => {
                let id = Operand::Constant(self.intern_string(id));
                self.emit(ByteOp::UnlockCg, vec![id], &statement.span);
            }
            StatementKind::Preload { asset } => {
                if let Some(asset) = self.asset_operand(asset) {
                    self.emit(ByteOp::PreloadAsset, vec![asset], &statement.span);
                }
            }
            StatementKind::OpenMenu { kind } => {
                let _ = kind;
                self.error(
                    DiagnosticCode::DeprecatedUiSyntax,
                    "'open'/'menu' is retired; use 'screen <name>;'.",
                    Some(statement.span.clone()),
                );
            }
            StatementKind::OpenScreen { screen } => {
                if !is_presentation_route(screen) {
                    self.error(
                        DiagnosticCode::InvalidUiBinding,
                        format!("screen '{screen}' is not a standard presentation route"),
                        Some(statement.span.clone()),
                    );
                    return;
                }
                let screen = Operand::Constant(self.intern_string(screen));
                self.emit(ByteOp::OpenScreen, vec![screen], &statement.span);
            }
            StatementKind::End => {
                self.cleanup_all_scopes(scopes, &statement.span);
                self.emit(ByteOp::End, Vec::new(), &statement.span);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_declaration(
        &mut self,
        value: &Value,
        mutable: bool,
        name: &str,
        declared_ty: Option<ModernType>,
        statement: &Statement,
        scopes: &mut [BTreeMap<String, Binding>],
        scene: &str,
    ) {
        let duplicate = scopes.last().is_none_or(|scope| scope.contains_key(name));
        if duplicate {
            self.error(
                DiagnosticCode::InvalidOperand,
                format!("duplicate variable '{name}' in this scope"),
                Some(statement.span.clone()),
            );
            return;
        }

        // Moving a Node is the only way a resource can change owners. It
        // generates no VM copy: the destination receives the exact same
        // deterministic scene-resource id and the source becomes moved.
        if let Value::Identifier { name: source, .. } = value
            && resolve_binding(scopes, source).is_some_and(|binding| binding.ty == ModernType::Node)
        {
            if declared_ty.is_some_and(|ty| ty != ModernType::Node) {
                self.error(
                    DiagnosticCode::InvalidOwnership,
                    format!("cannot move Node '{source}' into {name:?}"),
                    Some(statement.span.clone()),
                );
                return;
            }
            let Some(resource) = self.take_owned_node(source, scopes, &statement.span) else {
                return;
            };
            if let Some(scope) = scopes.last_mut() {
                scope.insert(
                    name.to_owned(),
                    Binding {
                        ty: ModernType::Node,
                        mutable,
                        operand: Operand::None,
                        resource: Some(resource),
                    },
                );
            }
            return;
        }

        let Some((value, value_ty)) = self.value_operand(value, scopes) else {
            return;
        };
        let ty = declared_ty.unwrap_or(value_ty);
        if ty == ModernType::Node {
            self.error(
                DiagnosticCode::InvalidOwnership,
                "a Node can only be created by 'let name = show …' or moved from another Node binding",
                Some(statement.span.clone()),
            );
            return;
        }
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
                    resource: None,
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_spawn(
        &mut self,
        mutable: bool,
        name: &str,
        content: &ShowContent,
        z: i32,
        statement: &Statement,
        scopes: &mut [BTreeMap<String, Binding>],
        scene: &str,
    ) {
        if scopes.last().is_none_or(|scope| scope.contains_key(name)) {
            self.error(
                DiagnosticCode::InvalidOperand,
                format!("duplicate variable '{name}' in this scope"),
                Some(statement.span.clone()),
            );
            return;
        }
        let resource = self.fresh_resource(scene, name);
        let id = Operand::Constant(self.intern_string(&resource.id));
        match content {
            ShowContent::Image { asset, position } => {
                let Some(asset) = self.asset_operand(asset) else {
                    return;
                };
                self.emit(
                    ByteOp::SpriteImage,
                    vec![
                        id,
                        asset,
                        Operand::Integer(i64::from(position.x_px)),
                        Operand::Integer(i64::from(position.y_px)),
                        Operand::Integer(i64::from(z)),
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
                        Operand::Integer(i64::from(z)),
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
                let text = Operand::Constant(self.intern_string(text));
                self.emit(
                    ByteOp::SpriteText,
                    vec![
                        id,
                        text,
                        Operand::Integer(i64::from(position.x_px)),
                        Operand::Integer(i64::from(position.y_px)),
                        Operand::Integer(i64::from(*size_px)),
                        Operand::Integer(i64::from(z)),
                    ],
                    &statement.span,
                );
            }
        }
        if let Some(scope) = scopes.last_mut() {
            scope.insert(
                name.to_owned(),
                Binding {
                    ty: ModernType::Node,
                    mutable,
                    operand: Operand::None,
                    resource: Some(resource),
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_borrow(
        &mut self,
        mutable: bool,
        owner: &str,
        alias: &str,
        body: &[Statement],
        statement: &Statement,
        scopes: &mut Vec<BTreeMap<String, Binding>>,
        scene: &str,
        source: &str,
    ) {
        let resource = match self.loan_owned_node(owner, mutable, scopes, &statement.span) {
            Some(resource) => resource,
            None => return,
        };
        scopes.push(BTreeMap::from([(
            alias.to_owned(),
            Binding {
                ty: ModernType::Node,
                mutable,
                operand: Operand::None,
                resource: Some(ResourceBinding {
                    id: resource.id,
                    drop_order: resource.drop_order,
                    ownership: ResourceOwnership::Borrowed { mutable },
                }),
            },
        )]));
        self.lower_block(source, body, scopes, scene);
        if let Some(mut borrow_scope) = scopes.pop() {
            // Borrow aliases themselves never own a resource, but the borrow
            // block is still a lexical scope and may contain freshly-owned
            // nodes. Drop those before making the owner available again.
            self.cleanup_scope(&mut borrow_scope, &statement.span);
        }
        self.restore_loan(owner, scopes);
    }

    /// Merges the ownership state of paths that both reach the same program
    /// point. Scalars use shared VM storage and do not need dataflow merging;
    /// Node ownership is affine and must agree exactly. When it does not, the
    /// source would otherwise have a path-dependent lifetime, so report one
    /// focused error and preserve the incoming model to avoid error cascades.
    fn merge_continuing_ownership(
        &mut self,
        before: &[BTreeMap<String, Binding>],
        left: &[BTreeMap<String, Binding>],
        right: &[BTreeMap<String, Binding>],
        span: &SourceSpan,
        construct: &str,
    ) -> Vec<BTreeMap<String, Binding>> {
        let mut merged = before.to_vec();
        for (scope_index, before_scope) in before.iter().enumerate() {
            let Some(left_scope) = left.get(scope_index) else {
                continue;
            };
            let Some(right_scope) = right.get(scope_index) else {
                continue;
            };
            let Some(merged_scope) = merged.get_mut(scope_index) else {
                continue;
            };
            for (name, before_binding) in before_scope {
                if before_binding.ty != ModernType::Node {
                    continue;
                }
                let Some(left_ownership) = left_scope
                    .get(name)
                    .and_then(|binding| binding.resource.as_ref())
                    .map(|resource| resource.ownership.clone())
                else {
                    continue;
                };
                let Some(right_ownership) = right_scope
                    .get(name)
                    .and_then(|binding| binding.resource.as_ref())
                    .map(|resource| resource.ownership.clone())
                else {
                    continue;
                };
                if left_ownership != right_ownership {
                    self.error(
                        DiagnosticCode::InvalidOwnership,
                        format!(
                            "{construct} ownership of Node '{name}' differs across continuing paths; move or drop it consistently"
                        ),
                        Some(span.clone()),
                    );
                    continue;
                }
                if let Some(resource) = merged_scope
                    .get_mut(name)
                    .and_then(|binding| binding.resource.as_mut())
                {
                    resource.ownership = left_ownership;
                }
            }
        }
        merged
    }

    fn with_scope(
        &mut self,
        scopes: &mut Vec<BTreeMap<String, Binding>>,
        span: &SourceSpan,
        action: impl FnOnce(&mut Self, &mut Vec<BTreeMap<String, Binding>>),
    ) {
        scopes.push(BTreeMap::new());
        action(self, scopes);
        if let Some(mut scope) = scopes.pop() {
            self.cleanup_scope(&mut scope, span);
        }
    }

    fn fresh_resource(&mut self, scene: &str, name: &str) -> ResourceBinding {
        let order = self.generated_resource;
        self.generated_resource = self.generated_resource.saturating_add(1);
        ResourceBinding {
            id: format!("aria:{scene}:{name}:{order}"),
            drop_order: order,
            ownership: ResourceOwnership::Owned,
        }
    }

    fn node_mut_borrow(
        &mut self,
        access: &NodeAccess,
        scopes: &[BTreeMap<String, Binding>],
    ) -> Option<String> {
        if !access.mutable {
            self.error(
                DiagnosticCode::InvalidBorrow,
                "scene mutation requires '&mut node'",
                Some(access.span.clone()),
            );
            return None;
        }
        let Some(binding) = resolve_binding(scopes, &access.name) else {
            self.error(
                DiagnosticCode::InvalidOwnership,
                format!("unknown Node binding '{}'", access.name),
                Some(access.span.clone()),
            );
            return None;
        };
        if binding.ty != ModernType::Node {
            self.error(
                DiagnosticCode::InvalidBorrow,
                format!("'{}' is not a Node", access.name),
                Some(access.span.clone()),
            );
            return None;
        }
        let Some(resource) = binding.resource else {
            self.error(
                DiagnosticCode::InvalidOwnership,
                format!("Node binding '{}' has no resource", access.name),
                Some(access.span.clone()),
            );
            return None;
        };
        match resource.ownership {
            ResourceOwnership::Owned if binding.mutable => Some(resource.id),
            ResourceOwnership::Owned => {
                self.error(
                    DiagnosticCode::InvalidBorrow,
                    format!(
                        "cannot mutably borrow immutable Node binding '{}'",
                        access.name
                    ),
                    Some(access.span.clone()),
                );
                None
            }
            ResourceOwnership::Borrowed { mutable: true } => Some(resource.id),
            ResourceOwnership::Borrowed { mutable: false } => {
                self.error(
                    DiagnosticCode::InvalidBorrow,
                    format!("borrow alias '{}' is immutable", access.name),
                    Some(access.span.clone()),
                );
                None
            }
            ResourceOwnership::Loaned { .. } => {
                self.error(
                    DiagnosticCode::BorrowConflict,
                    format!("Node '{}' is borrowed for this scope", access.name),
                    Some(access.span.clone()),
                );
                None
            }
            ResourceOwnership::Moved => {
                self.error(
                    DiagnosticCode::UseAfterMove,
                    format!("use of moved Node '{}'", access.name),
                    Some(access.span.clone()),
                );
                None
            }
        }
    }

    fn take_owned_node(
        &mut self,
        name: &str,
        scopes: &mut [BTreeMap<String, Binding>],
        span: &SourceSpan,
    ) -> Option<ResourceBinding> {
        let outcome = {
            let Some(binding) = resolve_binding_mut(scopes, name) else {
                return self.ownership_error(
                    DiagnosticCode::InvalidOwnership,
                    format!("unknown Node binding '{name}'"),
                    span,
                );
            };
            if binding.ty != ModernType::Node {
                return self.ownership_error(
                    DiagnosticCode::InvalidOwnership,
                    format!("'{name}' is not a Node"),
                    span,
                );
            }
            let Some(resource) = binding.resource.as_mut() else {
                return self.ownership_error(
                    DiagnosticCode::InvalidOwnership,
                    format!("Node binding '{name}' has no resource"),
                    span,
                );
            };
            match resource.ownership {
                ResourceOwnership::Owned => {
                    resource.ownership = ResourceOwnership::Moved;
                    Some(ResourceBinding {
                        id: resource.id.clone(),
                        drop_order: resource.drop_order,
                        ownership: ResourceOwnership::Owned,
                    })
                }
                ResourceOwnership::Moved => None,
                ResourceOwnership::Borrowed { .. } => None,
                ResourceOwnership::Loaned { .. } => None,
            }
        };
        if let Some(resource) = outcome {
            return Some(resource);
        }
        let code = resolve_binding(scopes, name)
            .and_then(|binding| binding.resource)
            .map(|resource| match resource.ownership {
                ResourceOwnership::Moved => DiagnosticCode::UseAfterMove,
                ResourceOwnership::Borrowed { .. } | ResourceOwnership::Loaned { .. } => {
                    DiagnosticCode::BorrowConflict
                }
                ResourceOwnership::Owned => DiagnosticCode::InvalidOwnership,
            })
            .unwrap_or(DiagnosticCode::InvalidOwnership);
        self.ownership_error(code, format!("cannot consume Node '{name}'"), span)
    }

    fn consume_owned_node(
        &mut self,
        name: &str,
        scopes: &mut [BTreeMap<String, Binding>],
        span: &SourceSpan,
    ) -> Option<String> {
        self.take_owned_node(name, scopes, span)
            .map(|resource| resource.id)
    }

    fn loan_owned_node(
        &mut self,
        name: &str,
        mutable: bool,
        scopes: &mut [BTreeMap<String, Binding>],
        span: &SourceSpan,
    ) -> Option<ResourceBinding> {
        let outcome = {
            let Some(binding) = resolve_binding_mut(scopes, name) else {
                return self.ownership_error(
                    DiagnosticCode::InvalidOwnership,
                    format!("unknown Node binding '{name}'"),
                    span,
                );
            };
            if binding.ty != ModernType::Node {
                return self.ownership_error(
                    DiagnosticCode::InvalidBorrow,
                    format!("'{name}' is not a Node"),
                    span,
                );
            }
            if mutable && !binding.mutable {
                return self.ownership_error(
                    DiagnosticCode::InvalidBorrow,
                    format!("cannot mutably borrow immutable Node binding '{name}'"),
                    span,
                );
            }
            let Some(resource) = binding.resource.as_mut() else {
                return self.ownership_error(
                    DiagnosticCode::InvalidOwnership,
                    format!("Node binding '{name}' has no resource"),
                    span,
                );
            };
            match resource.ownership {
                ResourceOwnership::Owned => {
                    let moved = ResourceBinding {
                        id: resource.id.clone(),
                        drop_order: resource.drop_order,
                        ownership: ResourceOwnership::Owned,
                    };
                    resource.ownership = ResourceOwnership::Loaned { mutable };
                    Some(moved)
                }
                _ => None,
            }
        };
        if outcome.is_some() {
            return outcome;
        }
        self.ownership_error(
            DiagnosticCode::BorrowConflict,
            format!("Node '{name}' is not available for a new borrow"),
            span,
        )
    }

    fn restore_loan(&mut self, name: &str, scopes: &mut [BTreeMap<String, Binding>]) {
        if let Some(binding) = resolve_binding_mut(scopes, name)
            && let Some(resource) = binding.resource.as_mut()
            && matches!(resource.ownership, ResourceOwnership::Loaned { .. })
        {
            resource.ownership = ResourceOwnership::Owned;
        }
    }

    fn cleanup_scope(&mut self, scope: &mut BTreeMap<String, Binding>, span: &SourceSpan) {
        let mut resources = scope
            .values_mut()
            .filter_map(|binding| binding.resource.as_mut())
            .filter_map(|resource| match resource.ownership {
                ResourceOwnership::Owned | ResourceOwnership::Loaned { .. } => {
                    resource.ownership = ResourceOwnership::Moved;
                    Some((resource.drop_order, resource.id.clone()))
                }
                ResourceOwnership::Moved | ResourceOwnership::Borrowed { .. } => None,
            })
            .collect::<Vec<_>>();
        resources.sort_by(|left, right| right.0.cmp(&left.0));
        for (_, id) in resources {
            let id = Operand::Constant(self.intern_string(&id));
            self.emit(ByteOp::SpriteRemove, vec![id], span);
        }
    }

    fn cleanup_all_scopes(&mut self, scopes: &mut [BTreeMap<String, Binding>], span: &SourceSpan) {
        for scope in scopes.iter_mut().rev() {
            self.cleanup_scope(scope, span);
        }
    }

    fn ownership_error<T>(
        &mut self,
        code: DiagnosticCode,
        message: impl Into<String>,
        span: &SourceSpan,
    ) -> Option<T> {
        self.error(code, message, Some(span.clone()));
        None
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
                // Bool storage uses the integer register file.  Comparing
                // that register to a Boolean literal would otherwise fall
                // through the VM's string comparison ("1" != "true"),
                // making every stored Bool look false at a branch. Keep the
                // source type Bool while lowering its runtime truth value to
                // the 0/1 representation used by SetInt.
                (binding.operand, "==", Operand::Integer(1))
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
                let (left_operand, right_operand) = if left_ty == ModernType::Bool {
                    (
                        Self::bool_runtime_operand(left_operand),
                        Self::bool_runtime_operand(right_operand),
                    )
                } else {
                    (left_operand, right_operand)
                };
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

    /// Runtime Bool bindings live in the integer register file (0 / 1).
    /// Literals retain their semantic Boolean form until a typed comparison
    /// needs to lower them beside such a binding.
    fn bool_runtime_operand(operand: Operand) -> Operand {
        match operand {
            Operand::Boolean(value) => Operand::Integer(i64::from(value)),
            operand => operand,
        }
    }

    fn expression_value(
        &mut self,
        expression: &Expression,
        scopes: &[BTreeMap<String, Binding>],
    ) -> Option<(Operand, ModernType)> {
        match &expression.kind {
            ExpressionKind::Literal(literal) => Some((self.literal_operand(literal), literal.ty())),
            ExpressionKind::Identifier(name) => match resolve_binding(scopes, name) {
                Some(binding) if binding.ty == ModernType::Node => {
                    self.error(
                        DiagnosticCode::InvalidOwnership,
                        format!("Node '{name}' cannot be used as an expression value"),
                        Some(expression.span.clone()),
                    );
                    None
                }
                Some(binding) => Some((binding.operand, binding.ty)),
                None => {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("unknown variable '{name}'"),
                        Some(expression.span.clone()),
                    );
                    None
                }
            },
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
            Value::Identifier { span, name } => match resolve_binding(scopes, name) {
                Some(binding) if binding.ty == ModernType::Node => {
                    self.error(
                        DiagnosticCode::InvalidOwnership,
                        format!("Node '{name}' must be moved into a new 'let' binding"),
                        Some(span.clone()),
                    );
                    None
                }
                Some(binding) => Some((binding.operand, binding.ty)),
                None => {
                    self.error(
                        DiagnosticCode::InvalidOperand,
                        format!("unknown variable '{name}'"),
                        Some(span.clone()),
                    );
                    None
                }
            },
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
            ModernType::Node => return,
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
            ModernType::Node => Operand::None,
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
        let label = format!("__aria_{prefix}_{}", self.generated_label);
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
                language_version: LanguageVersion::CURRENT,
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
            StatementKind::Borrow { body, .. } => {
                collect_scene_sites(body, source_scene, calls, transfers);
            }
            StatementKind::Say { .. }
            | StatementKind::Narrate { .. }
            | StatementKind::ClearDialogue
            | StatementKind::AwaitAdvance
            | StatementKind::Wait { .. }
            | StatementKind::Background { .. }
            | StatementKind::Spawn { .. }
            | StatementKind::Hide { .. }
            | StatementKind::Reveal { .. }
            | StatementKind::Drop { .. }
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
            | StatementKind::SetFlag { .. }
            | StatementKind::SetTextSpeed { .. }
            | StatementKind::SetAuto { .. }
            | StatementKind::SetSkip { .. }
            | StatementKind::SetLocale { .. }
            | StatementKind::SetTheme { .. }
            | StatementKind::SetTextBox { .. }
            | StatementKind::Tween { .. }
            | StatementKind::Effect { .. }
            | StatementKind::UnlockChapter { .. }
            | StatementKind::SetChapterProgress { .. }
            | StatementKind::UnlockCg { .. }
            | StatementKind::Preload { .. }
            | StatementKind::OpenMenu { .. }
            | StatementKind::OpenScreen { .. }
            | StatementKind::End => {}
        }
    }
}

/// True when every route through a statement list transfers control away from
/// its lexical continuation. This is deliberately side-effect-free: the
/// validation pass owns unreachable-code diagnostics, while lowering needs a
/// compact answer to merge ownership only for paths that can meet again.
fn block_definitely_exits(statements: &[Statement]) -> bool {
    statements.iter().any(statement_definitely_exits)
}

fn statement_definitely_exits(statement: &Statement) -> bool {
    match &statement.kind {
        StatementKind::End
        | StatementKind::Return
        | StatementKind::Jump { .. }
        | StatementKind::Choice { .. } => true,
        StatementKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            !else_branch.is_empty()
                && block_definitely_exits(then_branch)
                && block_definitely_exits(else_branch)
        }
        StatementKind::Borrow { body, .. } => block_definitely_exits(body),
        StatementKind::Say { .. }
        | StatementKind::Narrate { .. }
        | StatementKind::ClearDialogue
        | StatementKind::AwaitAdvance
        | StatementKind::Wait { .. }
        | StatementKind::Background { .. }
        | StatementKind::Spawn { .. }
        | StatementKind::Hide { .. }
        | StatementKind::Reveal { .. }
        | StatementKind::Drop { .. }
        | StatementKind::Move { .. }
        | StatementKind::Declare { .. }
        | StatementKind::Assign { .. }
        | StatementKind::AddAssign { .. }
        | StatementKind::While { .. }
        | StatementKind::Call { .. }
        | StatementKind::Play { .. }
        | StatementKind::Stop { .. }
        | StatementKind::Volume { .. }
        | StatementKind::Save { .. }
        | StatementKind::Load { .. }
        | StatementKind::SetFlag { .. }
        | StatementKind::SetTextSpeed { .. }
        | StatementKind::SetAuto { .. }
        | StatementKind::SetSkip { .. }
        | StatementKind::SetLocale { .. }
        | StatementKind::SetTheme { .. }
        | StatementKind::SetTextBox { .. }
        | StatementKind::Tween { .. }
        | StatementKind::Effect { .. }
        | StatementKind::UnlockChapter { .. }
        | StatementKind::SetChapterProgress { .. }
        | StatementKind::UnlockCg { .. }
        | StatementKind::Preload { .. }
        | StatementKind::OpenMenu { .. }
        | StatementKind::OpenScreen { .. } => false,
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

fn resolve_binding_mut<'a>(
    scopes: &'a mut [BTreeMap<String, Binding>],
    name: &str,
) -> Option<&'a mut Binding> {
    scopes
        .iter_mut()
        .rev()
        .find_map(|scope| scope.get_mut(name))
}

fn scene_label(name: &str) -> String {
    format!("scene:{name}")
}

fn is_presentation_route(route: &str) -> bool {
    matches!(
        route,
        "setup"
            | "title"
            | "demo_end"
            | "dialogue"
            | "pause"
            | "save"
            | "load"
            | "settings"
            | "backlog"
            | "chapter_select"
            | "gallery"
            // An interlude is a story-owned held surface. It deliberately
            // remains layout-free in Core, but is a standard semantic route
            // so strict scripts can save, log, and replay its silence.
            | "interlude"
            // A statement is a story-owned, automatic central line. It is
            // deliberately distinct from an interlude because ordinary
            // advance input must not release its authored duration.
            | "statement"
            // A chapter day card is a semantic presentation checkpoint. It
            // remains project-rendered, but is deliberately whitelisted so a
            // saved game can return to the same card rather than falling back
            // to dialogue on restore.
            | "day_card"
    )
}

fn type_name(ty: ModernType) -> &'static str {
    match ty {
        ModernType::Int => "Int",
        ModernType::Bool => "Bool",
        ModernType::String => "String",
        ModernType::Node => "Node",
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
            "aria;\n\
             entry start;\n\
             state route: Int = 0;\n\
             scene start {\n\
               background asset(\"#07131f\") with fade(200ms);\n\
               let mut ミオ = show image(asset(\"assets/mio.webp\")) at (760px, 86px) z 20;\n\
               say ミオ: \"海へ行こう。\";\n\
               choice { \"海\" => sea; \"駅\" => station; }\n\
             }\n\
             scene sea { let mut visits: Int = 0; visits += 1; if visits > 0 { play bgm asset(\"assets/sea.ogg\") loop; } end; }\n\
             scene station { end; }\n",
        );
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
        let program = output.program.unwrap();
        assert_eq!(program.language_version, LanguageVersion::CURRENT);
        program.validate().unwrap();
    }

    #[test]
    fn modern_semantics_reject_implicit_conversions_and_mutating_let() {
        let output = compile_script(
            "aria;\nentry start;\nscene start { let name: String = \"ミオ\"; name = \"別名\"; if name { end; } }\n",
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
            "aria;\nentry start;\nscene start { say \"海風\"; await advance; end; }\n",
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
    fn compiles_breath_waits_with_a_soft_release_operand_and_keeps_hard_waits_legacy_safe() {
        let output = compile_script(
            "aria;\nentry start;\nscene start { wait breath 300ms; wait 220ms; end; }\n",
        );
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
        let instructions = &output.program.unwrap().instructions;
        let delays = instructions
            .iter()
            .filter(|instruction| instruction.op == ByteOp::Delay)
            .collect::<Vec<_>>();
        assert_eq!(delays.len(), 2);
        assert_eq!(
            delays[0].operands,
            vec![Operand::Integer(300), Operand::Integer(160)]
        );
        assert_eq!(delays[1].operands, vec![Operand::Integer(220)]);
    }

    #[test]
    fn owned_nodes_move_borrow_and_drop_without_runtime_gc() {
        let output = compile_script(
            "aria;\n\
             entry start;\n\
             scene start {\n\
               let mut mio = show image(asset(\"assets/mio.webp\")) at (10px, 20px) z 3;\n\
               borrow mut mio as portrait {\n\
                 move &mut portrait to (30px, 40px);\n\
                 hide &mut portrait;\n\
               }\n\
               let outro = mio;\n\
               drop outro;\n\
               end;\n\
             }\n",
        );
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
        let program = output.program.unwrap();
        let removes = program
            .instructions
            .iter()
            .filter(|instruction| instruction.op == ByteOp::SpriteRemove)
            .count();
        assert_eq!(removes, 1, "moved/dropped node must release exactly once");
        program.validate().unwrap();
    }

    #[test]
    fn ownership_diagnostics_reject_use_after_move_and_borrow_conflicts() {
        let moved = compile_script(
            "aria;\nentry start;\nscene start {\n\
             let mut mio = show image(asset(\"assets/mio.webp\")) at (0px, 0px) z 1;\n\
             let outro = mio;\n\
             hide &mut mio;\n\
             end;\n}\n",
        );
        assert!(moved.has_errors());
        assert!(
            moved
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::UseAfterMove)
        );

        let borrowed = compile_script(
            "aria;\nentry start;\nscene start {\n\
             let mut mio = show image(asset(\"assets/mio.webp\")) at (0px, 0px) z 1;\n\
             borrow mut mio as portrait { hide &mut mio; }\n\
             end;\n}\n",
        );
        assert!(borrowed.has_errors());
        assert!(
            borrowed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::BorrowConflict)
        );
    }

    #[test]
    fn ownership_merges_exclusive_branches_and_rejects_path_dependent_lifetimes() {
        let both_drop = compile_script(
            "aria;\nentry start;\nstate mut route: Int = 0;\nscene start {\n\
             let mut mio = show image(asset(\"assets/mio.webp\")) at (0px, 0px) z 1;\n\
             if route == 0 { drop mio; } else { drop mio; }\n\
             end;\n}\n",
        );
        assert!(!both_drop.has_errors(), "{:#?}", both_drop.diagnostics);
        let program = both_drop.program.unwrap();
        assert_eq!(
            program
                .instructions
                .iter()
                .filter(|instruction| instruction.op == ByteOp::SpriteRemove)
                .count(),
            2,
            "each runtime branch owns and drops its copy of the control-flow path"
        );
        program.validate().unwrap();

        let divergent = compile_script(
            "aria;\nentry start;\nstate mut route: Int = 0;\nscene start {\n\
             let mut mio = show image(asset(\"assets/mio.webp\")) at (0px, 0px) z 1;\n\
             if route == 0 { drop mio; } else { hide &mut mio; }\n\
             end;\n}\n",
        );
        assert!(divergent.has_errors());
        assert!(divergent.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidOwnership
                && diagnostic.message.contains("conditional ownership")
        }));

        let loop_drop = compile_script(
            "aria;\nentry start;\nstate mut keep: Bool = false;\nscene start {\n\
             let mut mio = show image(asset(\"assets/mio.webp\")) at (0px, 0px) z 1;\n\
             while keep { drop mio; }\n\
             end;\n}\n",
        );
        assert!(loop_drop.has_errors());
        assert!(loop_drop.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidOwnership
                && diagnostic.message.contains("loop ownership")
        }));
    }

    #[test]
    fn borrow_blocks_drop_their_own_nodes_and_preserve_state_mutability() {
        let output = compile_script(
            "aria;\nentry start;\nstate mut route: Int = 0;\nscene start {\n\
             let mut mio = show image(asset(\"assets/mio.webp\")) at (0px, 0px) z 1;\n\
             borrow mut mio as portrait {\n\
               let temporary = show rect(0px, 0px, 16px, 16px, \"#fff\") z 2;\n\
               move &mut portrait to (1px, 2px);\n\
             }\n\
             route += 1;\n\
             end;\n}\n",
        );
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
        let program = output.program.unwrap();
        assert_eq!(
            program
                .instructions
                .iter()
                .filter(|instruction| instruction.op == ByteOp::SpriteRemove)
                .count(),
            2,
            "the temporary borrow-block node and the outer node each drop once"
        );

        let immutable_state = compile_script(
            "aria;\nentry start;\nstate route: Int = 0;\nscene start { route += 1; end; }\n",
        );
        assert!(immutable_state.has_errors());
        assert!(immutable_state.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("requires a mutable Int variable")
        }));
    }

    #[test]
    fn nodes_cannot_be_used_as_scalar_condition_values() {
        let output = compile_script(
            "aria;\nentry start;\nscene start {\n\
             let mio = show image(asset(\"assets/mio.webp\")) at (0px, 0px) z 1;\n\
             if mio == mio { end; } else { end; }\n\
             }\n",
        );
        assert!(output.has_errors());
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidOwnership
                && diagnostic
                    .message
                    .contains("cannot be used as an expression")
        }));
    }

    #[test]
    fn single_language_compiles_semantic_presentation_routes_into_ariac7() {
        let output = compile_script(
            "aria;\n\
             module test.ui;\n\
             entry start;\n\
             scene start { screen settings; end; }\n",
        );
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
        let program = output.program.unwrap();
        assert_eq!(program.language_version, LanguageVersion::CURRENT);
        assert!(
            program
                .instructions
                .iter()
                .any(|instruction| instruction.op == ByteOp::OpenScreen)
        );
        assert_eq!(
            CompiledProgram::decode(&program.encode().unwrap()).unwrap(),
            program
        );
    }

    #[test]
    fn single_language_reports_retired_visual_ui_syntax_and_invalid_routes() {
        let retired = compile_script(
            "aria;\n\
             ui_theme coast { string title \"Coast\"; }\n\
             ui_screen dialogue { slot dialogue; }\n\
             entry start;\n\
             scene start { theme umikaze; end; }\n",
        );
        assert!(retired.has_errors());
        assert!(retired.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DeprecatedUiSyntax
                && diagnostic.message.contains("retired")
        }));
        let declaration_lines = retired
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == DiagnosticCode::DeprecatedUiSyntax
                    && diagnostic.message.starts_with("visual UI declarations")
            })
            .filter_map(|diagnostic| diagnostic.span.as_ref().map(|span| span.line))
            .collect::<Vec<_>>();
        assert_eq!(declaration_lines, vec![2, 3]);

        let invalid = compile_script(
            "aria;\n\
             ui_theme coast { string title \"Coast\"; }\n\
             ui_screen dialogue { text value bind theme.missing; slot dialogue; }\n\
             entry start;\n\
             scene start { screen absent; end; }\n",
        );
        assert!(invalid.has_errors());
        assert!(invalid.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidUiBinding
                && diagnostic.message.contains("standard presentation route")
        }));
    }

    #[test]
    fn scenes_cannot_fall_through_and_imports_are_library_sources() {
        let output = compile(CompileInput {
            game_id: "jp.example.modern".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![
                SourceUnit {
                    logical_path: "scripts/main.aria".to_owned(),
                    source: "aria;\nuse \"./common.aria\";\nentry start;\nscene start { call helper; end; }\n".to_owned(),
                },
                SourceUnit {
                    logical_path: "scripts/common.aria".to_owned(),
                    source: "aria;\nscene helper { return; }\n".to_owned(),
                },
            ],
        });
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);

        let fallthrough = compile_script(
            "aria;\nentry start;\nscene start { narrate \"missing terminator\"; }\n",
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
            "aria;\n+             entry start;\n+             scene start { call helper; end; }\n+             scene helper { call helper; return; }\n",
        );
        assert!(output.has_errors());
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("recursive scene calls are not supported")
        }));

        let jump_to_return = compile_script(
            "aria;\nentry start;\nscene start { jump helper; }\nscene helper { return; }\n",
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
