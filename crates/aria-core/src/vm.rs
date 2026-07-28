use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bytecode::{ByteOp, CompiledProgram, Constant, EncodedInstruction, Operand};
use crate::input::{InputAction, InputSnapshot};
use crate::presentation::{
    ActionView, BacklogEntryView, ChapterView, ChoiceView, ConfirmationView, DialogueView,
    GalleryItemView, GameView, InterludeView, UI_VIEW_MODEL_SCHEMA, UiIntent, UiRoute, UiViewModel,
};
use crate::presentation_state::{PendingConfirmation, UiRuntimeState, UiViewport};
use crate::protocol::{
    AudioBus, AudioCommand, BlendMode, Color, DrawCommand, DrawStyle, LogicalSize, Rect,
    RuntimeCommand, SceneFrame, ScreenEffect, SpriteFit, StepOutput, TransitionFrame,
    TransitionKind,
};
use crate::text::{grapheme_count, grapheme_prefix, paginate_subtitles};

/// Schema 9 removes retired script-owned theme/textbox state. Deterministic
/// subtitle paging, replayable history targets, gallery viewing state, and
/// structured confirmation state remain persisted. Older saves are rejected
/// explicitly rather than silently guessing at a page boundary.
pub const VM_SNAPSHOT_SCHEMA: u32 = 9;
const DEFAULT_ROUTE: &str = "dialogue";
const MAX_INSTRUCTIONS_PER_TICK: usize = 100_000;
const BACKLOG_WINDOW_SIZE: usize = 48;
/// A malformed bytecode program must not turn an unbounded recursive Call
/// cycle into host-memory exhaustion. Aria rejects recursive calls at
/// compile time; this is the matching runtime guard for untrusted .ariac.
const MAX_CALL_DEPTH: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VmSnapshot {
    pub schema_version: u32,
    pub game_id: String,
    pub pc: u32,
    pub frame_number: u64,
    pub logical_time_ms: u64,
    pub last_input_sequence: Option<u64>,
    pub execution: ExecutionState,
    pub int_registers: BTreeMap<String, i64>,
    pub string_registers: BTreeMap<String, String>,
    pub call_stack: Vec<u32>,
    pub background: Option<BackgroundState>,
    pub sprites: BTreeMap<String, SpriteState>,
    pub text: TextState,
    pub choice: Option<ChoiceState>,
    pub transition: Option<TransitionState>,
    pub audio_tracks: BTreeMap<String, AudioTrackState>,
    pub bus_volumes: BTreeMap<AudioBus, f32>,
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
    #[serde(default)]
    pub persistent_flags: BTreeMap<String, bool>,
    #[serde(default)]
    pub chapters: BTreeMap<String, ChapterState>,
    #[serde(default)]
    pub unlocked_cgs: BTreeSet<String>,
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub read_texts: BTreeSet<String>,
    #[serde(default)]
    pub backlog: Vec<BacklogEntryState>,
    #[serde(default)]
    pub backlog_focused: usize,
    /// Per-page replay coordinates.  These are deliberately small trace
    /// references, never a serialized VM snapshot for every history row.
    #[serde(default)]
    pub backlog_targets: BTreeMap<String, BacklogTargetState>,
    /// A monotonically increasing source-line identity stays stable even when
    /// history is branched or trimmed.
    #[serde(default)]
    pub next_text_sequence: u64,
    /// Only player choices are required to reconstruct an earlier page. Text
    /// advances are deterministic and therefore do not grow this trace.
    #[serde(default)]
    pub narrative_trace: Vec<StoryTraceEvent>,
    #[serde(default)]
    pub auto_mode: AutoMode,
    #[serde(default)]
    pub skip_mode: SkipMode,
    #[serde(default)]
    pub auto_elapsed_ms: u32,
    #[serde(default)]
    pub auto_delay_ms: u32,
    #[serde(default)]
    pub settings: SettingsState,
    #[serde(default)]
    pub tweens: BTreeMap<String, TweenState>,
    #[serde(default)]
    pub effects: Vec<ScreenEffectState>,
    /// Persisted semantic route history and list positions. Layout, focus,
    /// controls, and the replay viewport are frontend/host-ephemeral.
    #[serde(default)]
    pub ui: UiRuntimeState,
    pub halted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacklogEntryState {
    pub id: String,
    pub speaker: Option<String>,
    pub text: String,
    pub locale: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacklogTargetState {
    pub text_id: String,
    pub page_index: usize,
    pub page_columns: usize,
    pub trace_position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryTraceEvent {
    pub choice_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterState {
    pub id: String,
    pub title: String,
    pub description: String,
    pub thumbnail: Option<String>,
    pub script: Option<String>,
    pub unlocked: bool,
    pub progress: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMode {
    Off,
    On,
}

impl Default for AutoMode {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipMode {
    Off,
    Read,
    All,
}

impl Default for SkipMode {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsState {
    pub text_speed_ms: u32,
    pub auto_delay_ms: u32,
    pub bgm_volume: f32,
    pub sound_effect_volume: f32,
    pub voice_volume: f32,
    pub fullscreen: bool,
    /// Text scale and contrast/motion choices belong to the same persisted
    /// user setting model as volume. The shared React presentation package
    /// consumes them on both Web and Tauri.
    #[serde(default = "default_text_scale")]
    pub text_scale: f32,
    /// Opacity of the fixed subtitle field. This is kept separate from
    /// high-contrast mode: one is a reading preference, the other is an
    /// accessibility override.
    #[serde(default = "default_text_opacity")]
    pub text_opacity: f32,
    #[serde(default)]
    pub high_contrast: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    /// Lets a reader retain the still photograph while suppressing the
    /// optional atmospheric grade and finite screen-effect overlays.
    #[serde(default = "default_stage_effects")]
    pub stage_effects: bool,
    /// Preserve the commercial VN convention of letting a player decide
    /// whether Skip may cross unread text. This is a preference, not the
    /// transient on/off state of the Skip command itself.
    #[serde(default)]
    pub skip_unread: bool,
}

const fn default_text_scale() -> f32 {
    1.0
}

const fn default_text_opacity() -> f32 {
    1.0
}

const fn default_stage_effects() -> bool {
    true
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            text_speed_ms: 24,
            auto_delay_ms: 900,
            bgm_volume: 1.0,
            sound_effect_volume: 1.0,
            voice_volume: 1.0,
            fullscreen: false,
            text_scale: 1.0,
            text_opacity: 1.0,
            high_contrast: false,
            reduced_motion: false,
            stage_effects: true,
            skip_unread: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TweenState {
    pub sprite_id: String,
    pub property: String,
    pub from: f32,
    pub to: f32,
    pub elapsed_ms: u32,
    pub duration_ms: u32,
    pub easing: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenEffectState {
    pub kind: ScreenEffectKind,
    pub color: Color,
    pub amount: f32,
    pub elapsed_ms: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenEffectKind {
    Tint,
    Flash,
    Shake,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionState {
    Running,
    WaitingForAdvance { clear_page: bool },
    WaitingForDelay { remaining_ms: u32 },
    WaitingForChoice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundState {
    pub asset: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpriteKind {
    Image,
    Rectangle,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteState {
    pub id: String,
    pub kind: SpriteKind,
    pub content: String,
    pub bounds: Rect,
    pub color: Color,
    pub font_size: f32,
    pub opacity: u8,
    pub z: i32,
    pub visible: bool,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub rotation_degrees: f32,
    #[serde(default)]
    pub tint: Color,
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextState {
    pub speaker: Option<String>,
    pub full_text: String,
    pub visible_graphemes: usize,
    pub reveal_elapsed_ms: u32,
    pub speed_ms: u32,
    #[serde(default)]
    pub text_id: Option<String>,
    /// Subtitle layout is captured when a line becomes current. This keeps a
    /// saved page and its history ID stable even if a host is resized later.
    #[serde(default = "default_subtitle_columns")]
    pub page_columns: usize,
    #[serde(default)]
    pub page_index: usize,
}

const fn default_subtitle_columns() -> usize {
    80
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            speaker: None,
            full_text: String::new(),
            visible_graphemes: 0,
            reveal_elapsed_ms: 0,
            speed_ms: 24,
            text_id: None,
            page_columns: default_subtitle_columns(),
            page_index: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOptionState {
    pub text: String,
    pub target: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceState {
    pub options: Vec<ChoiceOptionState>,
    pub focused: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionState {
    pub kind: TransitionKind,
    pub elapsed_ms: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrackState {
    pub bus: AudioBus,
    pub id: String,
    pub asset: String,
    pub looping: bool,
    pub volume: f32,
}

#[derive(Debug, Clone)]
pub struct Vm {
    program: CompiledProgram,
    logical_size: LogicalSize,
    state: VmSnapshot,
    pending_audio: Vec<AudioCommand>,
    pending_runtime: Vec<RuntimeCommand>,
    /// Internal replay intentionally rebuilds state from a choice trace. It
    /// must not materialize a second temporary history while doing so.
    suppress_backlog_recording: bool,
}

impl Vm {
    pub fn new(program: CompiledProgram, logical_size: LogicalSize) -> Result<Self, VmError> {
        program
            .validate()
            .map_err(|error| VmError::InvalidProgram(error.to_string()))?;
        if logical_size.width == 0 || logical_size.height == 0 {
            return Err(VmError::InvalidLogicalSize);
        }
        let game_id = program.game_id.clone();
        Ok(Self {
            program,
            logical_size,
            state: VmSnapshot {
                schema_version: VM_SNAPSHOT_SCHEMA,
                game_id,
                pc: 0,
                frame_number: 0,
                logical_time_ms: 0,
                last_input_sequence: None,
                execution: ExecutionState::Running,
                int_registers: BTreeMap::new(),
                string_registers: BTreeMap::new(),
                call_stack: Vec::new(),
                background: None,
                sprites: BTreeMap::new(),
                text: TextState::default(),
                choice: None,
                transition: None,
                audio_tracks: BTreeMap::new(),
                bus_volumes: BTreeMap::from([
                    (AudioBus::Bgm, 1.0),
                    (AudioBus::SoundEffect, 1.0),
                    (AudioBus::Voice, 1.0),
                ]),
                flags: BTreeMap::new(),
                persistent_flags: BTreeMap::new(),
                chapters: BTreeMap::new(),
                unlocked_cgs: BTreeSet::new(),
                locale: "ja-JP".to_owned(),
                read_texts: BTreeSet::new(),
                backlog: Vec::new(),
                backlog_focused: 0,
                backlog_targets: BTreeMap::new(),
                next_text_sequence: 0,
                narrative_trace: Vec::new(),
                auto_mode: AutoMode::Off,
                skip_mode: SkipMode::Off,
                auto_elapsed_ms: 0,
                auto_delay_ms: 900,
                settings: SettingsState::default(),
                tweens: BTreeMap::new(),
                effects: Vec::new(),
                ui: UiRuntimeState {
                    route: DEFAULT_ROUTE.to_owned(),
                    viewport: UiViewport {
                        width: logical_size.width,
                        height: logical_size.height,
                        ..UiViewport::default()
                    },
                    ..UiRuntimeState::default()
                },
                halted: false,
            },
            pending_audio: Vec::new(),
            pending_runtime: Vec::new(),
            suppress_backlog_recording: false,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> VmSnapshot {
        self.state.clone()
    }

    pub fn restore(&mut self, mut snapshot: VmSnapshot) -> Result<(), VmError> {
        if snapshot.schema_version != VM_SNAPSHOT_SCHEMA {
            return Err(VmError::UnsupportedSnapshot(snapshot.schema_version));
        }
        if snapshot.game_id != self.program.game_id {
            return Err(VmError::WrongGame {
                expected: self.program.game_id.clone(),
                actual: snapshot.game_id,
            });
        }
        if usize::try_from(snapshot.pc)
            .ok()
            .is_none_or(|pc| pc >= self.program.instructions.len())
            && !snapshot.halted
        {
            return Err(VmError::InvalidSnapshotPc(snapshot.pc));
        }
        // Input sequence numbers identify a delivery stream, not game state.
        // A save can be loaded by a fresh Player whose sequence starts again at
        // one, so preserving this value would reject the first resumed input.
        snapshot.last_input_sequence = None;
        if !snapshot.ui.viewport.valid() {
            snapshot.ui.viewport = UiViewport {
                width: self.logical_size.width,
                height: self.logical_size.height,
                ..UiViewport::default()
            };
        }
        if !is_standard_route(&snapshot.ui.route) {
            snapshot.ui.route = DEFAULT_ROUTE.to_owned();
        }
        snapshot
            .ui
            .route_stack
            .retain(|route| is_standard_route(route));
        let confirmation_is_valid = snapshot
            .ui
            .confirmation
            .as_ref()
            .is_some_and(|confirmation| {
                matches!(confirmation.action.as_str(), "reset" | "quit")
                    || (confirmation.action == "resume_backlog"
                        && confirmation
                            .resume_id
                            .as_ref()
                            .is_some_and(|id| snapshot.backlog_targets.contains_key(id)))
            });
        if snapshot.ui.route == "confirm" && !confirmation_is_valid {
            snapshot.ui.route = snapshot
                .ui
                .route_stack
                .pop()
                .filter(|route| is_standard_route(route))
                .unwrap_or_else(|| DEFAULT_ROUTE.to_owned());
        }
        if snapshot.ui.route != "confirm" {
            snapshot.ui.confirmation = None;
        }
        if snapshot.ui.route != "gallery"
            || snapshot
                .ui
                .gallery_viewer
                .as_ref()
                .is_some_and(|id| !snapshot.unlocked_cgs.contains(id))
        {
            snapshot.ui.gallery_viewer = None;
        }
        snapshot.settings.text_scale = snapshot.settings.text_scale.clamp(0.85, 1.35);
        snapshot.settings.text_opacity = snapshot.settings.text_opacity.clamp(0.72, 1.0);
        let restored_audio = snapshot
            .bus_volumes
            .iter()
            .map(|(bus, volume)| AudioCommand::SetBusVolume {
                bus: *bus,
                volume: *volume,
                fade_ms: 0,
            })
            .chain(
                snapshot
                    .audio_tracks
                    .values()
                    .map(|track| AudioCommand::Play {
                        bus: track.bus,
                        id: track.id.clone(),
                        asset: track.asset.clone(),
                        looping: track.looping,
                        volume: track.volume,
                        fade_in_ms: 0,
                    }),
            )
            .collect();
        self.state = snapshot;
        self.pending_audio = restored_audio;
        self.pending_runtime.clear();
        Ok(())
    }

    /// Changes the locale without exposing a host locale or filesystem to the
    /// deterministic VM.  Text localization tables are supplied by the
    /// project/package layer and represented as ordinary values.
    pub fn set_locale(&mut self, locale: impl Into<String>) {
        self.state.locale = locale.into();
    }

    pub fn set_auto_mode(&mut self, enabled: bool) {
        self.state.auto_mode = if enabled { AutoMode::On } else { AutoMode::Off };
        self.state.auto_elapsed_ms = 0;
    }

    pub fn set_skip_mode(&mut self, mode: SkipMode) {
        self.state.skip_mode = mode;
    }

    #[must_use]
    pub fn backlog(&self) -> &[BacklogEntryState] {
        &self.state.backlog
    }

    #[must_use]
    pub fn read_rate(&self) -> f32 {
        let total = self.state.backlog.len();
        if total == 0 {
            0.0
        } else {
            self.state.read_texts.len().min(total) as f32 / total as f32
        }
    }

    #[must_use]
    pub fn logical_size(&self) -> LogicalSize {
        self.logical_size
    }

    /// The narrative authoring canvas remains fixed, but the composed
    /// frame is expressed in the replayed viewport's dp coordinates. Native
    /// and Web therefore render a 390dp phone UI at 390 logical units rather
    /// than shrinking 44dp targets inside a 1280-wide letterbox.
    #[must_use]
    fn frame_logical_size(&self) -> LogicalSize {
        let viewport = self.state.ui.viewport;
        let dimension = |value: f32, fallback: u32| {
            if value.is_finite() {
                value.round().clamp(1.0, 16_384.0) as u32
            } else {
                fallback.max(1)
            }
        };
        LogicalSize {
            width: dimension(viewport.logical_width(), self.logical_size.width),
            height: dimension(viewport.logical_height(), self.logical_size.height),
        }
    }

    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.state.halted
    }

    pub fn step(&mut self, input: &InputSnapshot) -> Result<StepOutput, VmError> {
        if let Some(previous) = self.state.last_input_sequence
            && input.sequence <= previous
        {
            return Err(VmError::NonMonotonicInput {
                previous,
                received: input.sequence,
            });
        }
        self.state.last_input_sequence = Some(input.sequence);
        if !input.scroll_delta_y.is_finite() {
            return Err(VmError::InvalidScrollDelta);
        }
        if let Some(viewport) = input.viewport {
            if !viewport.valid() {
                return Err(VmError::InvalidViewport);
            }
            self.state.ui.viewport = viewport;
        }
        self.state.frame_number = self.state.frame_number.saturating_add(1);
        self.state.logical_time_ms = self
            .state
            .logical_time_ms
            .saturating_add(u64::from(input.delta_ms));
        self.update_transition(input.delta_ms);
        self.update_effects(input.delta_ms);
        self.update_tweens(input.delta_ms);
        self.update_typewriter(
            input.delta_ms,
            input.is_held(InputAction::Skip) || self.state.skip_mode != SkipMode::Off,
        );

        if input.is_pressed(InputAction::ToggleAuto) {
            self.set_auto_mode(self.state.auto_mode == AutoMode::Off);
        }
        if input.is_pressed(InputAction::ToggleSkip) {
            self.state.skip_mode = if self.state.skip_mode == SkipMode::Off {
                self.preferred_skip_mode()
            } else {
                SkipMode::Off
            };
        }
        if input.is_pressed(InputAction::QuickSave) {
            self.pending_runtime.push(RuntimeCommand::QuickSave);
        }
        if input.is_pressed(InputAction::QuickLoad) {
            self.pending_runtime.push(RuntimeCommand::QuickLoad);
        }

        let mut route_changed = self.apply_presentation_intents(input)?;
        if !route_changed && input.is_pressed(InputAction::Menu) {
            self.open_screen("pause");
            route_changed = true;
        } else if !route_changed && input.is_pressed(InputAction::Cancel) {
            if self.screen_dismisses_on_cancel() {
                self.close_screen();
                route_changed = true;
            } else if self.is_reading_surface() {
                self.open_screen("pause");
                route_changed = true;
            }
        } else if !route_changed && input.is_pressed(InputAction::OpenBacklog) {
            self.open_screen("backlog");
            route_changed = true;
        }

        // A custom story surface can be either a choice checkpoint (the day
        // card) or a timed reading beat (an interlude or an automatic
        // statement). Only presentation overlays bypass the ordinary
        // waiting-state update. Treating every non-dialogue route as UI used
        // to freeze a `wait` authored on a story-owned surface indefinitely.
        if !route_changed
            && (self.state.execution == ExecutionState::WaitingForChoice
                || (self.state.ui.route != DEFAULT_ROUTE && !self.is_reading_surface()))
        {
            if input.intents.is_empty() {
                self.handle_direct_presentation_input(input)?;
            }
            self.run_running_instructions()?;
        } else if !route_changed {
            self.handle_waiting_input(input)?;
            self.run_running_instructions()?;
        }
        let scene = self.scene_frame();
        let view = self.build_view_model();
        Ok(StepOutput {
            scene,
            view,
            audio: std::mem::take(&mut self.pending_audio),
            runtime: std::mem::take(&mut self.pending_runtime),
            halted: self.state.halted,
        })
    }

    fn run_running_instructions(&mut self) -> Result<(), VmError> {
        let mut executed = 0;
        while !self.state.halted && self.state.execution == ExecutionState::Running {
            if executed >= MAX_INSTRUCTIONS_PER_TICK {
                return Err(self.error_at_pc(VmErrorKind::InstructionBudgetExceeded));
            }
            let pc = usize::try_from(self.state.pc)
                .map_err(|_| self.error_at_pc(VmErrorKind::ProgramCounterOutOfRange))?;
            let instruction = self
                .program
                .instructions
                .get(pc)
                .cloned()
                .ok_or_else(|| self.error_at_pc(VmErrorKind::ProgramCounterOutOfRange))?;
            self.state.pc = self.state.pc.saturating_add(1);
            self.execute(&instruction)?;
            executed += 1;
        }
        Ok(())
    }

    fn update_transition(&mut self, delta_ms: u32) {
        if let Some(transition) = &mut self.state.transition {
            transition.elapsed_ms = transition.elapsed_ms.saturating_add(delta_ms);
            if transition.elapsed_ms >= transition.duration_ms {
                self.state.transition = None;
            }
        }
    }

    fn update_typewriter(&mut self, delta_ms: u32, skip_requested: bool) {
        let total = self.current_page_grapheme_count();
        if self.state.text.visible_graphemes >= total {
            return;
        }
        let skip_current = skip_requested
            && (self.state.skip_mode == SkipMode::All
                || (self.state.skip_mode == SkipMode::Read
                    && self
                        .state
                        .text
                        .text_id
                        .as_ref()
                        .is_some_and(|id| self.state.read_texts.contains(id))));
        if skip_current || self.state.text.speed_ms == 0 {
            self.state.text.visible_graphemes = total;
            self.record_current_page_if_needed();
            return;
        }
        self.state.text.reveal_elapsed_ms =
            self.state.text.reveal_elapsed_ms.saturating_add(delta_ms);
        let reveal = self.state.text.reveal_elapsed_ms / self.state.text.speed_ms;
        self.state.text.reveal_elapsed_ms %= self.state.text.speed_ms;
        self.state.text.visible_graphemes = self
            .state
            .text
            .visible_graphemes
            .saturating_add(reveal as usize)
            .min(total);
        if self.state.text.visible_graphemes >= total {
            self.record_current_page_if_needed();
        }
    }

    fn handle_waiting_input(&mut self, input: &InputSnapshot) -> Result<(), VmError> {
        match self.state.execution.clone() {
            ExecutionState::Running => {}
            ExecutionState::WaitingForDelay { remaining_ms } => {
                let mut remaining = remaining_ms.saturating_sub(input.delta_ms);
                // Auto preserves the interlude's pause but never makes a
                // first-read 1.2s hold feel like a stalled auto route. A
                // short revisit hold remains untouched; a longer one is
                // capped to a quiet 320ms from the next VM update.
                if self.state.ui.route == "interlude" && self.state.auto_mode == AutoMode::On {
                    remaining = remaining.min(320);
                }
                // A cinematic beat may be skipped by the player's normal
                // skip mode as well as a physically held skip input. Its text
                // is already complete and recorded, so this never drops
                // unread prose on the floor. An interlude is intentionally
                // reader-releasable; a statement is not. Its normal advance,
                // confirm, pointer, and gamepad-A inputs are ignored until
                // the authored duration has elapsed.
                if remaining == 0
                    || input.is_held(InputAction::Skip)
                    || ((self.state.ui.route == "interlude" || self.state.ui.route == "statement")
                        && self.state.skip_mode != SkipMode::Off)
                    || (self.state.ui.route == "interlude"
                        && (input.is_pressed(InputAction::Advance)
                            || input.is_pressed(InputAction::Confirm)
                            || input.pointer.is_some_and(|pointer| pointer.primary_pressed)))
                {
                    self.state.execution = ExecutionState::Running;
                } else {
                    self.state.execution = ExecutionState::WaitingForDelay {
                        remaining_ms: remaining,
                    };
                }
            }
            ExecutionState::WaitingForAdvance { .. } => {
                self.state.auto_elapsed_ms =
                    self.state.auto_elapsed_ms.saturating_add(input.delta_ms);
                let activated = input.is_pressed(InputAction::Advance)
                    || input.is_pressed(InputAction::Confirm)
                    || input.pointer.is_some_and(|pointer| pointer.primary_pressed)
                    || input.is_held(InputAction::Skip)
                    || self.skip_current_line()
                    || (self.state.auto_mode == AutoMode::On
                        && self.state.auto_elapsed_ms >= self.state.auto_delay_ms);
                if activated {
                    self.advance_dialogue();
                }
            }
            // A frontend dispatches choice IDs through `UiIntent`; this arm
            // only documents the outer routing guard for direct terminals.
            ExecutionState::WaitingForChoice => {}
        }
        Ok(())
    }

    /// Applies frontend-owned semantic events. The accepted IDs are stable
    /// contract values, never DOM IDs or renderer node keys.
    fn apply_presentation_intents(&mut self, input: &InputSnapshot) -> Result<bool, VmError> {
        let mut route_changed = false;
        for intent in &input.intents {
            match intent {
                UiIntent::Activate { id } => {
                    route_changed |= self.activate_presentation_action(id)?;
                }
                UiIntent::OpenRoute { route } => {
                    let normalized = self.resolve_screen(route);
                    if normalized == DEFAULT_ROUTE && normalize_screen_name(route) != DEFAULT_ROUTE
                    {
                        return Err(VmError::InvalidUiIntent(format!(
                            "unknown presentation route '{route}'"
                        )));
                    }
                    self.open_screen(&normalized);
                    route_changed = true;
                }
                UiIntent::Dismiss => {
                    if self.screen_dismisses_on_cancel() {
                        self.close_screen();
                        route_changed = true;
                    }
                }
                UiIntent::SetSetting { name, value } => {
                    if !value.is_finite() || !is_setting_name(name) {
                        return Err(VmError::InvalidUiIntent(format!(
                            "invalid setting '{name}'"
                        )));
                    }
                    self.set_setting_value(name, *value);
                }
                UiIntent::ToggleSetting { name } => {
                    if !matches!(
                        name.as_str(),
                        "fullscreen"
                            | "high_contrast"
                            | "reduced_motion"
                            | "stage_effects"
                            | "skip_unread"
                    ) {
                        return Err(VmError::InvalidUiIntent(format!(
                            "invalid toggle setting '{name}'"
                        )));
                    }
                    self.toggle_setting(name);
                }
                UiIntent::Scroll { region, delta_y } => {
                    if !delta_y.is_finite() {
                        return Err(VmError::InvalidScrollDelta);
                    }
                    self.apply_presentation_scroll(region, *delta_y)?;
                }
            }
        }
        Ok(route_changed)
    }

    /// Keeps command-line and direct native input usable alongside the Tauri
    /// player. Unlike a UI tree this path has no coordinates or visual focus
    /// state.
    fn handle_direct_presentation_input(&mut self, input: &InputSnapshot) -> Result<(), VmError> {
        self.apply_ui_scroll(input.scroll_delta_y);
        if self.state.ui.route == "gallery" && self.state.ui.gallery_viewer.is_some() {
            if input.is_pressed(InputAction::NavigateLeft)
                || input.is_pressed(InputAction::NavigateUp)
            {
                self.move_gallery_viewer(-1);
            } else if input.is_pressed(InputAction::NavigateRight)
                || input.is_pressed(InputAction::NavigateDown)
            {
                self.move_gallery_viewer(1);
            }
            return Ok(());
        }
        if self.state.execution != ExecutionState::WaitingForChoice {
            return Ok(());
        }
        let selected = {
            let Some(choice) = &mut self.state.choice else {
                return Err(self.error_at_pc(VmErrorKind::MissingChoiceState));
            };
            if !choice.options.is_empty()
                && (input.is_pressed(InputAction::NavigateUp)
                    || input.is_pressed(InputAction::NavigateLeft))
            {
                choice.focused = choice.focused.saturating_sub(1);
            } else if !choice.options.is_empty()
                && (input.is_pressed(InputAction::NavigateDown)
                    || input.is_pressed(InputAction::NavigateRight))
            {
                choice.focused = (choice.focused + 1).min(choice.options.len().saturating_sub(1));
            }
            choice.focused
        };
        if input.is_pressed(InputAction::Confirm) || input.is_pressed(InputAction::Advance) {
            self.select_choice(selected)?;
        }
        Ok(())
    }

    fn activate_presentation_action(&mut self, id: &str) -> Result<bool, VmError> {
        if id == "dialogue.advance" {
            self.advance_dialogue();
            return Ok(false);
        }
        if id == "interlude.advance" {
            if self.state.ui.route != "interlude" {
                return Err(VmError::InvalidUiIntent(
                    "interlude advance is only valid on an interlude".to_owned(),
                ));
            }
            // The interlude line was revealed and logged on entry. A reader's
            // input only releases the authored held duration.
            if matches!(self.state.execution, ExecutionState::WaitingForDelay { .. }) {
                self.state.execution = ExecutionState::Running;
            }
            return Ok(false);
        }
        if id == "chrome.menu" {
            self.open_screen("pause");
            return Ok(true);
        }
        if id == "chrome.backlog" {
            self.open_screen("backlog");
            return Ok(true);
        }
        if id == "dismiss" {
            if self.screen_dismisses_on_cancel() {
                self.close_screen();
                return Ok(true);
            }
            return Ok(false);
        }
        if let Some(index) = id.strip_prefix("choice:") {
            let index = index
                .parse::<usize>()
                .map_err(|_| VmError::InvalidUiIntent(format!("invalid choice action '{id}'")))?;
            // Presentation input is asynchronous: a second Enter/click can
            // arrive after the first semantic choice has been queued but
            // before React has committed the next story surface.  The first
            // activation owns the choice; subsequent copies of that now-stale
            // action must not turn a perfectly valid chapter transition into
            // a fatal missing-choice error.  Keep malformed indices while a
            // live choice exists diagnostic, but make a no-longer-live choice
            // action an idempotent no-op.
            if self.state.execution != ExecutionState::WaitingForChoice
                || self.state.choice.is_none()
            {
                return Ok(false);
            }
            self.select_choice(index)?;
            return Ok(false);
        }
        if let Some(route) = id.strip_prefix("route:") {
            let resolved = self.resolve_screen(route);
            if resolved == DEFAULT_ROUTE && normalize_screen_name(route) != DEFAULT_ROUTE {
                return Err(VmError::InvalidUiIntent(format!(
                    "unknown presentation route '{route}'"
                )));
            }
            self.open_screen(&resolved);
            return Ok(true);
        }
        if let Some(slot) = presentation_manual_save_slot(id) {
            self.pending_runtime.push(RuntimeCommand::Save { slot });
            self.close_all_screens();
            return Ok(true);
        }
        if let Some(slot) = presentation_load_slot(id) {
            self.pending_runtime.push(RuntimeCommand::Load { slot });
            return Ok(false);
        }
        match id {
            "menu.save" => self.open_screen("save"),
            "menu.load" => self.open_screen("load"),
            "menu.backlog" => self.open_screen("backlog"),
            "menu.gallery" => self.open_screen("gallery"),
            "menu.settings" => self.open_screen("settings"),
            "menu.auto" => {
                self.set_auto_mode(self.state.auto_mode == AutoMode::Off);
                self.close_all_screens();
                return Ok(true);
            }
            "menu.skip" => {
                self.state.skip_mode = if self.state.skip_mode == SkipMode::Off {
                    self.preferred_skip_mode()
                } else {
                    SkipMode::Off
                };
                self.close_all_screens();
                return Ok(true);
            }
            "menu.reset" | "menu.title" => {
                self.open_confirmation("reset", None);
                return Ok(true);
            }
            "menu.quit" => {
                self.open_confirmation("quit", None);
                return Ok(true);
            }
            "confirm.accept" => return self.confirm_pending_action(),
            "confirm.cancel" => {
                self.state.ui.confirmation = None;
                self.close_screen();
                return Ok(true);
            }
            "rmenu.close" => {
                // Secondary click is a local sheet dismissal. It must not
                // discard the whole overlay stack (or a full-screen CG behind
                // the current confirmation) in one gesture.
                self.close_screen();
                return Ok(true);
            }
            _ => {
                if let Some(entry_id) = id.strip_prefix("backlog:") {
                    let Some(index) = self
                        .state
                        .backlog
                        .iter()
                        .position(|entry| entry.id == entry_id)
                    else {
                        return Err(VmError::InvalidUiIntent(format!(
                            "unknown backlog entry '{entry_id}'"
                        )));
                    };
                    self.state.backlog_focused = index;
                    self.open_confirmation("resume_backlog", Some(entry_id.to_owned()));
                    return Ok(true);
                }
                if let Some(chapter_id) = id.strip_prefix("chapter:") {
                    let Some(chapter) = self.state.chapters.get(chapter_id) else {
                        return Err(VmError::InvalidUiIntent(format!(
                            "unknown chapter '{chapter_id}'"
                        )));
                    };
                    if chapter.unlocked {
                        self.close_screen();
                        return Ok(true);
                    }
                    return Ok(false);
                }
                if let Some(cg_id) = id.strip_prefix("gallery:") {
                    if self.state.unlocked_cgs.contains(cg_id) {
                        self.state.ui.gallery_viewer = Some(cg_id.to_owned());
                        return Ok(true);
                    }
                    return Err(VmError::InvalidUiIntent(format!(
                        "unknown gallery item '{cg_id}'"
                    )));
                }
                if id == "gallery.close" {
                    self.state.ui.gallery_viewer = None;
                    return Ok(true);
                }
                if id == "gallery.previous" || id == "gallery.next" {
                    self.move_gallery_viewer(if id == "gallery.previous" { -1 } else { 1 });
                    return Ok(true);
                }
                return Err(VmError::InvalidUiIntent(format!(
                    "unknown presentation action '{id}'"
                )));
            }
        }
        Ok(matches!(
            id,
            "menu.save" | "menu.load" | "menu.backlog" | "menu.gallery" | "menu.settings"
        ))
    }

    fn resolve_screen(&self, requested: &str) -> String {
        let route = normalize_screen_name(requested);
        if is_standard_route(route) {
            route.to_owned()
        } else {
            DEFAULT_ROUTE.to_owned()
        }
    }

    /// A story `screen` instruction changes the base route, so old transient
    /// sheet history must not leak into the next scene.
    fn present_screen(&mut self, requested: &str) {
        let previous = self.state.ui.route.clone();
        self.state.ui.route = self.resolve_screen(requested);
        // Atomic story fields own their one complete line. Do not let it
        // briefly masquerade as a subtitle while a following day card,
        // transition, or authored delay is being prepared. Besides being a
        // visual leak, retaining its text id would make a prose backlog
        // target ambiguous during deterministic replay.
        if is_atomic_story_route(&previous) && previous != self.state.ui.route {
            self.clear_text();
        }
        self.state.ui.route_stack.clear();
        self.state.ui.confirmation = None;
        self.state.ui.gallery_viewer = None;
        if self.state.ui.route != "interlude" {
            self.state.ui.interlude_first_visit = false;
        }
        let current = self.state.ui.route.clone();
        self.start_ui_transition(&previous, &current);
    }

    /// A button/menu action creates a temporary sheet above the current
    /// route. This small navigation stack is serializable UI state.
    fn open_screen(&mut self, requested: &str) {
        let next = self.resolve_screen(requested);
        if next == self.state.ui.route {
            return;
        }
        let previous = self.state.ui.route.clone();
        self.state.ui.route_stack.push(previous.clone());
        self.state.ui.route = next;
        let current = self.state.ui.route.clone();
        self.start_ui_transition(&previous, &current);
    }

    fn close_screen(&mut self) {
        let previous = self.state.ui.route.clone();
        if previous == "gallery" && self.state.ui.gallery_viewer.take().is_some() {
            // The image viewer is the topmost gallery layer. Closing it
            // returns to the grid without closing the gallery itself.
            return;
        }
        if previous == "confirm" {
            self.state.ui.confirmation = None;
        }
        self.state.ui.route = self
            .state
            .ui
            .route_stack
            .pop()
            .filter(|route| is_standard_route(route))
            .unwrap_or_else(|| DEFAULT_ROUTE.to_owned());
        let current = self.state.ui.route.clone();
        self.start_ui_transition(&previous, &current);
    }

    fn close_all_screens(&mut self) {
        let previous = self.state.ui.route.clone();
        let base = self
            .state
            .ui
            .route_stack
            .first()
            .cloned()
            .filter(|route| is_standard_route(route))
            .unwrap_or_else(|| {
                if is_standard_route(&previous) && !UiRoute::parse(&previous).is_overlay() {
                    previous.clone()
                } else {
                    DEFAULT_ROUTE.to_owned()
                }
            });
        self.state.ui.route = base;
        self.state.ui.route_stack.clear();
        self.state.ui.confirmation = None;
        self.state.ui.gallery_viewer = None;
        let current = self.state.ui.route.clone();
        self.start_ui_transition(&previous, &current);
    }

    fn open_confirmation(&mut self, action: &str, resume_id: Option<String>) {
        self.state.ui.confirmation = Some(PendingConfirmation {
            action: action.to_owned(),
            resume_id,
        });
        self.open_screen("confirm");
    }

    fn confirm_pending_action(&mut self) -> Result<bool, VmError> {
        let confirmation = self
            .state
            .ui
            .confirmation
            .take()
            .ok_or_else(|| VmError::InvalidUiIntent("no pending confirmation".to_owned()))?;
        match confirmation.action.as_str() {
            "reset" => {
                self.close_all_screens();
                self.pending_runtime.push(RuntimeCommand::ReturnToTitle);
            }
            "quit" => {
                self.close_all_screens();
                self.pending_runtime.push(RuntimeCommand::Quit);
            }
            "resume_backlog" => {
                let resume_id = confirmation.resume_id.ok_or_else(|| {
                    VmError::InvalidUiIntent("backlog confirmation has no target".to_owned())
                })?;
                self.resume_from_backlog(&resume_id)?;
            }
            _ => {
                return Err(VmError::InvalidUiIntent(format!(
                    "unknown confirmation action '{}'",
                    confirmation.action
                )));
            }
        }
        Ok(true)
    }

    fn move_gallery_viewer(&mut self, delta: isize) {
        let unlocked = self.state.unlocked_cgs.iter().cloned().collect::<Vec<_>>();
        if unlocked.is_empty() {
            self.state.ui.gallery_viewer = None;
            return;
        }
        let current = self
            .state
            .ui
            .gallery_viewer
            .as_ref()
            .and_then(|id| unlocked.iter().position(|candidate| candidate == id))
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(unlocked.len() as isize) as usize;
        self.state.ui.gallery_viewer = unlocked.get(next).cloned();
    }

    /// Rebuilds story state from the small choice trace stored with a backlog
    /// page.  This deliberately does not deserialize a line-by-line VM image:
    /// save size remains proportional to meaningful choices rather than the
    /// number of pages a reader has seen.
    fn resume_from_backlog(&mut self, resume_id: &str) -> Result<(), VmError> {
        let target_index = self
            .state
            .backlog
            .iter()
            .position(|entry| entry.id == resume_id)
            .ok_or_else(|| {
                VmError::InvalidUiIntent(format!("unknown backlog entry '{resume_id}'"))
            })?;
        let target = self
            .state
            .backlog_targets
            .get(resume_id)
            .cloned()
            .ok_or_else(|| {
                VmError::InvalidUiIntent(format!(
                    "backlog entry '{resume_id}' has no replay target"
                ))
            })?;
        if target.trace_position > self.state.narrative_trace.len() {
            return Err(VmError::InvalidUiIntent(format!(
                "backlog entry '{resume_id}' has an invalid replay trace"
            )));
        }

        let preserved_history = self.state.backlog[..=target_index].to_vec();
        let preserved_targets = self
            .state
            .backlog_targets
            .iter()
            .filter(|(id, _)| preserved_history.iter().any(|entry| &entry.id == *id))
            .map(|(id, target)| (id.clone(), target.clone()))
            .collect::<BTreeMap<_, _>>();
        let trace = self.state.narrative_trace[..target.trace_position].to_vec();
        let persistent_flags = self.state.persistent_flags.clone();
        let chapters = self.state.chapters.clone();
        let unlocked_cgs = self.state.unlocked_cgs.clone();
        let read_texts = self.state.read_texts.clone();
        let settings = self.state.settings.clone();
        let bus_volumes = self.state.bus_volumes.clone();
        let auto_mode = self.state.auto_mode;
        let skip_mode = self.state.skip_mode;
        let viewport = self.state.ui.viewport;

        let mut replay = Vm::new(self.program.clone(), self.logical_size)?;
        replay.suppress_backlog_recording = true;
        replay.state.ui.viewport = viewport;
        replay.state.settings = settings.clone();
        replay.state.text.speed_ms = settings.text_speed_ms;
        replay.state.auto_delay_ms = settings.auto_delay_ms;
        replay.state.bus_volumes = bus_volumes.clone();
        replay.run_running_instructions()?;

        let mut trace_index = 0_usize;
        const MAX_REPLAY_TURNS: usize = 1_000_000;
        let mut reached_target = false;
        for _ in 0..MAX_REPLAY_TURNS {
            if replay.state.text.text_id.as_deref() == Some(target.text_id.as_str())
                && (matches!(
                    replay.state.execution,
                    ExecutionState::WaitingForAdvance { .. }
                ) || (is_atomic_story_route(&replay.state.ui.route)
                    && matches!(
                        replay.state.execution,
                        ExecutionState::WaitingForDelay { .. }
                    )))
            {
                replay.state.text.page_columns = target.page_columns.max(1);
                let page_count = replay.subtitle_pages().len();
                if target.page_index >= page_count {
                    return Err(VmError::InvalidUiIntent(format!(
                        "backlog entry '{resume_id}' points outside its subtitle pages"
                    )));
                }
                replay.state.text.page_index = target.page_index;
                replay.state.text.visible_graphemes = replay.current_page_grapheme_count();
                replay.state.text.reveal_elapsed_ms = 0;
                reached_target = true;
                break;
            }

            match replay.state.execution.clone() {
                ExecutionState::Running => replay.run_running_instructions()?,
                ExecutionState::WaitingForDelay { .. } => {
                    // Delay contains no branching state. The reader has asked
                    // to return to prose, so it is safe to consume internally.
                    replay.state.execution = ExecutionState::Running;
                    replay.run_running_instructions()?;
                }
                ExecutionState::WaitingForAdvance { .. } => {
                    replay.state.text.visible_graphemes = replay.current_page_grapheme_count();
                    replay.advance_dialogue();
                    replay.run_running_instructions()?;
                }
                ExecutionState::WaitingForChoice => {
                    let event = trace.get(trace_index).ok_or_else(|| {
                        VmError::InvalidUiIntent(format!(
                            "backlog entry '{resume_id}' needs a choice absent from its trace"
                        ))
                    })?;
                    replay.select_choice(event.choice_index)?;
                    trace_index = trace_index.saturating_add(1);
                    replay.run_running_instructions()?;
                }
            }
            if replay.state.halted {
                break;
            }
        }
        if !reached_target {
            return Err(VmError::InvalidUiIntent(format!(
                "backlog entry '{resume_id}' cannot be reconstructed from its trace"
            )));
        }

        replay.state.backlog = preserved_history;
        replay.state.backlog_targets = preserved_targets;
        replay.state.backlog_focused = target_index;
        replay.state.narrative_trace = trace;
        replay.state.persistent_flags = persistent_flags;
        replay.state.chapters = chapters;
        replay.state.unlocked_cgs = unlocked_cgs;
        replay.state.read_texts = read_texts;
        replay.state.settings = settings;
        replay.state.bus_volumes = bus_volumes;
        replay.state.auto_mode = auto_mode;
        replay.state.skip_mode = skip_mode;
        replay.state.text.speed_ms = replay.state.settings.text_speed_ms;
        replay.state.auto_delay_ms = replay.state.settings.auto_delay_ms;
        replay.state.ui.route_stack.clear();
        replay.state.ui.confirmation = None;
        replay.state.ui.gallery_viewer = None;
        replay.state.ui.scroll_offsets.clear();
        self.state = replay.state;
        self.pending_audio = replay.pending_audio;
        self.pending_runtime.clear();
        Ok(())
    }

    fn screen_dismisses_on_cancel(&self) -> bool {
        self.state.ui.route != DEFAULT_ROUTE
            && UiRoute::parse(&self.state.ui.route).is_overlay()
            && !self.is_reading_surface()
    }

    fn is_reading_surface(&self) -> bool {
        self.state.ui.route == DEFAULT_ROUTE
            // Day cards are a held part of the story, not an overlay. Treat
            // Cancel exactly like the dialogue surface so it opens RMenu and
            // never dismisses the chapter checkpoint by accident.
            || self.state.ui.route == "day_card"
            // Interludes and statements are story time, not overlays. Cancel
            // therefore opens RMenu and save/restore retains their exact
            // held beat.
            || self.state.ui.route == "interlude"
            || self.state.ui.route == "statement"
            || (self.state.ui.route == "chapter_select"
                && self.state.choice.is_none()
                && !self.state.text.full_text.is_empty())
    }

    fn start_ui_transition(&mut self, from: &str, to: &str) {
        if from == to || self.state.settings.reduced_motion {
            self.state.transition = None;
            return;
        }
        // Surface choreography is implemented in CSS. Core emits only a
        // conservative scene fade so replay remains deterministic without
        // prescribing component motion or geometry.
        let kind = TransitionKind::Fade;
        self.state.transition = Some(TransitionState {
            kind,
            elapsed_ms: 0,
            duration_ms: 180,
        });
    }

    fn apply_ui_scroll(&mut self, delta_y: f32) {
        if delta_y.abs() < f32::EPSILON {
            return;
        }
        let (key, maximum) = match self.state.ui.route.as_str() {
            "backlog" => ("backlog", self.state.backlog.len().saturating_sub(8) as f32),
            "gallery" => (
                "gallery",
                self.state.unlocked_cgs.len().saturating_sub(12) as f32,
            ),
            "chapter_select" => (
                "chapters",
                self.state.chapters.len().saturating_sub(8) as f32,
            ),
            _ => return,
        };
        let offset = self
            .state
            .ui
            .scroll_offsets
            .entry(key.to_owned())
            .or_insert(0.0);
        *offset = (*offset + delta_y / 48.0).clamp(0.0, maximum);
    }

    fn backlog_window_start(&self) -> usize {
        let maximum_start = self.state.backlog.len().saturating_sub(BACKLOG_WINDOW_SIZE);
        (self
            .state
            .ui
            .scroll_offsets
            .get("backlog")
            .copied()
            .unwrap_or(0.0)
            .max(0.0)
            .floor() as usize)
            .min(maximum_start)
    }

    fn apply_presentation_scroll(&mut self, region: &str, delta_y: f32) -> Result<(), VmError> {
        let maximum = match region {
            "backlog" => self.state.backlog.len().saturating_sub(8) as f32,
            "gallery" => self.state.unlocked_cgs.len().saturating_sub(12) as f32,
            "chapters" => self.state.chapters.len().saturating_sub(8) as f32,
            _ => {
                return Err(VmError::InvalidUiIntent(format!(
                    "unknown scroll region '{region}'"
                )));
            }
        };
        let offset = self
            .state
            .ui
            .scroll_offsets
            .entry(region.to_owned())
            .or_insert(0.0);
        *offset = (*offset + delta_y / 48.0).clamp(0.0, maximum);
        Ok(())
    }

    fn advance_dialogue(&mut self) {
        if let ExecutionState::WaitingForAdvance { clear_page } = self.state.execution {
            let total = self.current_page_grapheme_count();
            if self.state.text.visible_graphemes < total {
                self.state.text.visible_graphemes = total;
                self.record_current_page_if_needed();
            } else if self.advance_to_next_subtitle_page() {
                self.state.auto_elapsed_ms = 0;
            } else {
                if clear_page {
                    self.clear_text();
                }
                self.mark_current_text_read();
                self.state.auto_elapsed_ms = 0;
                self.state.execution = ExecutionState::Running;
            }
        }
    }

    fn subtitle_pages(&self) -> Vec<String> {
        paginate_subtitles(
            &self.state.text.full_text,
            self.state.text.page_columns.max(1),
            2,
        )
    }

    fn current_subtitle_page(&self) -> String {
        let pages = self.subtitle_pages();
        pages
            .get(
                self.state
                    .text
                    .page_index
                    .min(pages.len().saturating_sub(1)),
            )
            .cloned()
            .unwrap_or_default()
    }

    fn current_page_grapheme_count(&self) -> usize {
        grapheme_count(&self.current_subtitle_page())
    }

    fn current_page_id(&self) -> Option<String> {
        self.state.text.text_id.as_ref().map(|text_id| {
            format!(
                "{text_id}:page-{}",
                self.state.text.page_index.saturating_add(1)
            )
        })
    }

    fn advance_to_next_subtitle_page(&mut self) -> bool {
        let page_count = self.subtitle_pages().len();
        if self.state.text.page_index.saturating_add(1) >= page_count {
            return false;
        }
        self.state.text.page_index = self.state.text.page_index.saturating_add(1);
        self.state.text.visible_graphemes = 0;
        self.state.text.reveal_elapsed_ms = 0;
        true
    }

    /// Adds precisely the page that became readable.  This is intentionally
    /// called at page completion, rather than when a source `text` instruction
    /// starts, so history is a record of what the reader actually reached.
    fn record_current_page_if_needed(&mut self) {
        if self.suppress_backlog_recording {
            return;
        }
        let Some(page_id) = self.current_page_id() else {
            return;
        };
        if self.state.backlog.iter().any(|entry| entry.id == page_id) {
            return;
        }
        let pages = self.subtitle_pages();
        let page_index = self
            .state
            .text
            .page_index
            .min(pages.len().saturating_sub(1));
        let Some(text) = pages.get(page_index).cloned() else {
            return;
        };
        let Some(text_id) = self.state.text.text_id.clone() else {
            return;
        };
        self.state.backlog.push(BacklogEntryState {
            id: page_id.clone(),
            speaker: self.state.text.speaker.clone(),
            text,
            locale: self.state.locale.clone(),
            timestamp_ms: self.state.logical_time_ms,
        });
        self.state.backlog_targets.insert(
            page_id,
            BacklogTargetState {
                text_id,
                page_index,
                page_columns: self.state.text.page_columns,
                trace_position: self.state.narrative_trace.len(),
            },
        );
        self.state.backlog_focused = self.state.backlog.len().saturating_sub(1);
    }

    fn select_choice(&mut self, index: usize) -> Result<(), VmError> {
        let Some(choice) = &self.state.choice else {
            return Err(self.error_at_pc(VmErrorKind::MissingChoiceState));
        };
        let Some(option) = choice.options.get(index) else {
            return Err(self.error_at_pc(VmErrorKind::MissingChoiceState));
        };
        self.state
            .int_registers
            .insert("choice".to_owned(), index as i64 + 1);
        // Choices are the only nondeterministic story input. Capturing their
        // indices is enough to replay an earlier history page without keeping
        // an ever-growing VM snapshot beside every subtitle.
        self.state.narrative_trace.push(StoryTraceEvent {
            choice_index: index,
        });
        self.state.pc = option.target;
        self.state.choice = None;
        self.state.execution = ExecutionState::Running;
        // A model choice hands control back to narrative code. Any title,
        // chapter, or sheet route is therefore explicitly re-opened by a
        // following `screen` statement rather than leaking across scenes.
        self.close_screen();
        Ok(())
    }

    fn set_setting_value(&mut self, name: &str, value: f32) {
        match name {
            "text_speed_ms" => {
                self.state.settings.text_speed_ms = value.clamp(0.0, 200.0) as u32;
                self.state.text.speed_ms = self.state.settings.text_speed_ms;
            }
            "auto_delay_ms" => {
                self.state.settings.auto_delay_ms = value.clamp(100.0, 10_000.0) as u32;
                self.state.auto_delay_ms = self.state.settings.auto_delay_ms;
            }
            "bgm_volume" => self.set_setting_volume_to(AudioBus::Bgm, value),
            "sound_effect_volume" => self.set_setting_volume_to(AudioBus::SoundEffect, value),
            "voice_volume" => self.set_setting_volume_to(AudioBus::Voice, value),
            "text_scale" => self.state.settings.text_scale = value.clamp(0.85, 1.35),
            "text_opacity" => self.state.settings.text_opacity = value.clamp(0.72, 1.0),
            _ => {}
        }
    }

    fn toggle_setting(&mut self, name: &str) {
        match name {
            "fullscreen" => self.state.settings.fullscreen = !self.state.settings.fullscreen,
            "high_contrast" => {
                self.state.settings.high_contrast = !self.state.settings.high_contrast;
            }
            "reduced_motion" => {
                self.state.settings.reduced_motion = !self.state.settings.reduced_motion;
            }
            "stage_effects" => {
                self.state.settings.stage_effects = !self.state.settings.stage_effects;
            }
            "skip_unread" => {
                self.state.settings.skip_unread = !self.state.settings.skip_unread;
                // A running skip reflects the newly selected policy without
                // requiring the reader to turn Skip off and on again.
                if self.state.skip_mode != SkipMode::Off {
                    self.state.skip_mode = self.preferred_skip_mode();
                }
            }
            _ => {}
        }
    }

    fn preferred_skip_mode(&self) -> SkipMode {
        if self.state.settings.skip_unread {
            SkipMode::All
        } else {
            SkipMode::Read
        }
    }

    fn set_setting_volume(&mut self, bus: AudioBus, delta: f32) {
        let current = self.state.bus_volumes.get(&bus).copied().unwrap_or(1.0);
        let volume = (current + delta).clamp(0.0, 1.0);
        self.state.bus_volumes.insert(bus, volume);
        match bus {
            AudioBus::Bgm => {
                self.state.settings.bgm_volume = volume;
            }
            AudioBus::SoundEffect => {
                self.state.settings.sound_effect_volume = volume;
            }
            AudioBus::Voice => {
                self.state.settings.voice_volume = volume;
            }
        }
        self.pending_audio.push(AudioCommand::SetBusVolume {
            bus,
            volume,
            fade_ms: 120,
        });
    }

    fn set_setting_volume_to(&mut self, bus: AudioBus, value: f32) {
        let current = self.state.bus_volumes.get(&bus).copied().unwrap_or(1.0);
        self.set_setting_volume(bus, value.clamp(0.0, 1.0) - current);
    }

    fn mark_current_text_read(&mut self) {
        if let Some(id) = self.state.text.text_id.clone() {
            self.state.read_texts.insert(id);
        }
    }

    fn skip_current_line(&self) -> bool {
        match self.state.skip_mode {
            SkipMode::Off => false,
            SkipMode::All => true,
            SkipMode::Read => self
                .state
                .text
                .text_id
                .as_ref()
                .is_some_and(|id| self.state.read_texts.contains(id)),
        }
    }

    fn update_effects(&mut self, delta_ms: u32) {
        for effect in &mut self.state.effects {
            effect.elapsed_ms = effect.elapsed_ms.saturating_add(delta_ms);
        }
        self.state
            .effects
            .retain(|effect| effect.elapsed_ms < effect.duration_ms.max(1));
    }

    fn update_tweens(&mut self, delta_ms: u32) {
        let mut finished = Vec::new();
        for (key, tween) in &mut self.state.tweens {
            tween.elapsed_ms = tween.elapsed_ms.saturating_add(delta_ms);
            let raw = if tween.duration_ms == 0 {
                1.0
            } else {
                (tween.elapsed_ms as f32 / tween.duration_ms as f32).clamp(0.0, 1.0)
            };
            let progress = ease(raw, &tween.easing);
            if let Some(sprite) = self.state.sprites.get_mut(&tween.sprite_id) {
                let value = tween.from + (tween.to - tween.from) * progress;
                match tween.property.as_str() {
                    "x" => sprite.bounds.x = value,
                    "y" => sprite.bounds.y = value,
                    "opacity" => sprite.opacity = value.clamp(0.0, 255.0) as u8,
                    "scale" => sprite.scale = value.max(0.0),
                    "rotation" | "rotation_degrees" => sprite.rotation_degrees = value,
                    _ => {}
                }
            }
            if raw >= 1.0 {
                finished.push(key.clone());
            }
        }
        for key in finished {
            self.state.tweens.remove(&key);
        }
    }

    fn execute(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        match instruction.op {
            ByteOp::Nop => {}
            ByteOp::Text => {
                let speaker = match instruction.operands.first() {
                    Some(Operand::None) | None => None,
                    Some(value) => Some(self.string(value)?),
                };
                let text = self.string(self.operand(instruction, 1)?)?;
                let text_id = format!("text-{}", self.state.next_text_sequence);
                self.state.next_text_sequence = self.state.next_text_sequence.saturating_add(1);
                self.state.text = TextState {
                    speaker,
                    full_text: text,
                    visible_graphemes: 0,
                    reveal_elapsed_ms: 0,
                    speed_ms: self.state.text.speed_ms,
                    text_id: Some(text_id),
                    page_columns: subtitle_columns(
                        self.state.ui.viewport,
                        self.state.settings.text_scale,
                    ),
                    page_index: 0,
                };
                // Unlike a subtitle, an atomic story field is a complete
                // short statement over a still frame. Reveal it atomically
                // so it never starts a typewriter rAF loop, then place that
                // exact page in the backlog before the authored hold begins.
                if is_atomic_story_route(&self.state.ui.route) {
                    self.state.text.visible_graphemes = self.current_page_grapheme_count();
                    self.record_current_page_if_needed();
                    self.mark_current_text_read();
                }
            }
            ByteOp::WaitAdvance => {
                let clear_page = self.boolean(self.operand(instruction, 0)?)?;
                self.state.execution = ExecutionState::WaitingForAdvance { clear_page };
            }
            ByteOp::TextClear => self.clear_text(),
            ByteOp::Delay => {
                let duration = self.integer(self.operand(instruction, 0)?)?.max(0) as u32;
                // UmiKaze interludes use a long first hold and a short
                // revisit hold. Capture that distinction at the semantic
                // delay boundary so a save keeps the visual blank/fade rhythm
                // without the host owning a second timer.
                if self.state.ui.route == "interlude" {
                    self.state.ui.interlude_first_visit = duration >= 600;
                }
                if duration > 0 {
                    self.state.execution = ExecutionState::WaitingForDelay {
                        remaining_ms: duration,
                    };
                }
            }
            ByteOp::Jump => self.state.pc = self.address(self.operand(instruction, 0)?)?,
            ByteOp::JumpIfFalse => {
                let left = self.value(self.operand(instruction, 0)?)?;
                let comparator = self.string(self.operand(instruction, 1)?)?;
                let right = self.value(self.operand(instruction, 2)?)?;
                if !compare(&left, &comparator, &right) {
                    self.state.pc = self.address(self.operand(instruction, 3)?)?;
                }
            }
            ByteOp::Call => {
                let target = self.address(self.operand(instruction, 0)?)?;
                if self.state.call_stack.len() >= MAX_CALL_DEPTH {
                    return Err(self.error_at_pc(VmErrorKind::CallDepthExceeded));
                }
                self.state.call_stack.push(self.state.pc);
                self.state.pc = target;
            }
            ByteOp::Return => {
                let address = self
                    .state
                    .call_stack
                    .pop()
                    .ok_or_else(|| self.error_at_pc(VmErrorKind::ReturnWithoutCaller))?;
                self.state.pc = address;
            }
            ByteOp::SetInt => {
                let target = self.int_register_name(self.operand(instruction, 0)?)?;
                let value = self.integer(self.operand(instruction, 1)?)?;
                self.state.int_registers.insert(target, value);
            }
            ByteOp::AddInt => {
                let target = self.int_register_name(self.operand(instruction, 0)?)?;
                let value = self.integer(self.operand(instruction, 1)?)?;
                let sign = self.integer(self.operand(instruction, 2)?)?;
                let current = self.state.int_registers.get(&target).copied().unwrap_or(0);
                self.state
                    .int_registers
                    .insert(target, current.saturating_add(value.saturating_mul(sign)));
            }
            ByteOp::SetString => {
                let target = self.string_register_name(self.operand(instruction, 0)?)?;
                let value = self.string(self.operand(instruction, 1)?)?;
                self.state.string_registers.insert(target, value);
            }
            ByteOp::Background => {
                let asset = self.string(self.operand(instruction, 0)?)?;
                let duration = self.integer(self.operand(instruction, 1)?)?.max(0) as u32;
                self.state.background = Some(BackgroundState { asset });
                if duration > 0 {
                    self.state.transition = Some(TransitionState {
                        kind: TransitionKind::Fade,
                        elapsed_ms: 0,
                        duration_ms: duration,
                    });
                }
            }
            ByteOp::SpriteImage => self.execute_sprite_image(instruction)?,
            ByteOp::SpriteRect => self.execute_sprite_rect(instruction)?,
            ByteOp::SpriteText => self.execute_sprite_text(instruction)?,
            ByteOp::SpriteRemove => {
                let id = self.string(self.operand(instruction, 0)?)?;
                if id == "-1" || id.eq_ignore_ascii_case("all") {
                    self.state.sprites.clear();
                } else {
                    self.state.sprites.remove(&id);
                }
            }
            ByteOp::SpriteVisibility => {
                let id = self.string(self.operand(instruction, 0)?)?;
                let visible = self.boolean(self.operand(instruction, 1)?)?;
                if let Some(sprite) = self.state.sprites.get_mut(&id) {
                    sprite.visible = visible;
                }
            }
            ByteOp::SpriteMove => {
                let id = self.string(self.operand(instruction, 0)?)?;
                let x = self.float(self.operand(instruction, 1)?)?;
                let y = self.float(self.operand(instruction, 2)?)?;
                if let Some(sprite) = self.state.sprites.get_mut(&id) {
                    sprite.bounds.x = x;
                    sprite.bounds.y = y;
                }
            }
            ByteOp::PresentChoice => self.execute_choice(instruction)?,
            ByteOp::PlayAudio => self.execute_audio_play(instruction)?,
            ByteOp::StopAudio => self.execute_audio_stop(instruction)?,
            ByteOp::SetVolume => self.execute_volume(instruction)?,
            ByteOp::BeginTransition => {
                let kind = parse_transition(&self.string(self.operand(instruction, 0)?)?);
                let duration = self.integer(self.operand(instruction, 1)?)?.max(1) as u32;
                self.state.transition = Some(TransitionState {
                    kind,
                    elapsed_ms: 0,
                    duration_ms: duration,
                });
            }
            ByteOp::Save => {
                let slot = self.integer(self.operand(instruction, 0)?)?.max(0) as u32;
                self.pending_runtime.push(RuntimeCommand::Save { slot });
            }
            ByteOp::Load => {
                let slot = self.integer(self.operand(instruction, 0)?)?.max(0) as u32;
                self.pending_runtime.push(RuntimeCommand::Load { slot });
            }
            ByteOp::End => self.state.halted = true,
            ByteOp::SetFlag => {
                let name = self.string(self.operand(instruction, 0)?)?;
                let value = self.boolean(self.operand(instruction, 1)?)?;
                self.state.flags.insert(name, value);
            }
            ByteOp::SetPersistentFlag => {
                let name = self.string(self.operand(instruction, 0)?)?;
                let value = self.boolean(self.operand(instruction, 1)?)?;
                self.state.persistent_flags.insert(name.clone(), value);
                self.state.flags.insert(name, value);
            }
            ByteOp::SetTextSpeed => {
                let speed = self.integer(self.operand(instruction, 0)?)?.max(0) as u32;
                self.state.text.speed_ms = speed;
                self.state.settings.text_speed_ms = speed;
            }
            ByteOp::SetAutoMode => {
                self.set_auto_mode(self.boolean(self.operand(instruction, 0)?)?);
            }
            ByteOp::SetSkipMode => {
                let mode = self.string(self.operand(instruction, 0)?)?;
                self.state.skip_mode = parse_skip_mode(&mode);
            }
            ByteOp::SetLocale => {
                self.state.locale = self.string(self.operand(instruction, 0)?)?;
            }
            ByteOp::TweenSprite => self.execute_tween(instruction)?,
            ByteOp::ScreenEffect => self.execute_screen_effect(instruction)?,
            ByteOp::UnlockChapter => {
                let id = self.string(self.operand(instruction, 0)?)?;
                let title = id.clone();
                let progress = self.integer(self.operand(instruction, 1)?)?.clamp(0, 100) as u8;
                let chapter = self
                    .state
                    .chapters
                    .entry(id.clone())
                    .or_insert(ChapterState {
                        id,
                        title,
                        description: String::new(),
                        thumbnail: None,
                        script: None,
                        unlocked: false,
                        progress: 0,
                    });
                chapter.unlocked = true;
                chapter.progress = chapter.progress.max(progress);
            }
            ByteOp::SetChapterProgress => {
                let id = self.string(self.operand(instruction, 0)?)?;
                let progress = self.integer(self.operand(instruction, 1)?)?.clamp(0, 100) as u8;
                let chapter = self
                    .state
                    .chapters
                    .entry(id.clone())
                    .or_insert(ChapterState {
                        id: id.clone(),
                        title: id,
                        description: String::new(),
                        thumbnail: None,
                        script: None,
                        unlocked: false,
                        progress: 0,
                    });
                chapter.progress = progress;
            }
            ByteOp::UnlockCg => {
                let id = self.string(self.operand(instruction, 0)?)?;
                self.state.unlocked_cgs.insert(id);
            }
            ByteOp::PreloadAsset => {
                self.pending_runtime.push(RuntimeCommand::PreloadAsset {
                    asset: self.string(self.operand(instruction, 0)?)?,
                });
            }
            ByteOp::OpenScreen => {
                let screen = self.string(self.operand(instruction, 0)?)?;
                let route = normalize_screen_name(&screen).to_owned();
                self.present_screen(&route);
            }
            ByteOp::GetFlag => {
                let name = self.string(self.operand(instruction, 0)?)?;
                let target = self.int_register_name(self.operand(instruction, 1)?)?;
                let value = self.state.flags.get(&name).copied().unwrap_or(false);
                self.state.int_registers.insert(target, i64::from(value));
            }
        }
        Ok(())
    }

    fn execute_tween(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let id = self.string(self.operand(instruction, 0)?)?;
        let property = self.string(self.operand(instruction, 1)?)?;
        let target = self.float(self.operand(instruction, 2)?)?;
        let duration = self.integer(self.operand(instruction, 3)?)?.max(0) as u32;
        let easing = self.string(self.operand(instruction, 4)?)?;
        let Some(sprite) = self.state.sprites.get(&id) else {
            return Ok(());
        };
        let from = match property.as_str() {
            "x" => sprite.bounds.x,
            "y" => sprite.bounds.y,
            "opacity" => f32::from(sprite.opacity),
            "scale" => sprite.scale,
            "rotation" | "rotation_degrees" => sprite.rotation_degrees,
            _ => target,
        };
        self.state.tweens.insert(
            format!("{id}:{property}"),
            TweenState {
                sprite_id: id,
                property,
                from,
                to: target,
                elapsed_ms: 0,
                duration_ms: duration,
                easing,
            },
        );
        Ok(())
    }

    fn execute_screen_effect(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let kind = parse_effect_kind(&self.string(self.operand(instruction, 0)?)?);
        let color =
            parse_color(&self.string(self.operand(instruction, 1)?)?).unwrap_or(Color::WHITE);
        let amount = self.float(self.operand(instruction, 2)?)?.clamp(0.0, 255.0);
        let duration = self.integer(self.operand(instruction, 3)?)?.max(1) as u32;
        let _axis = self.string(self.operand(instruction, 4)?)?;
        self.state.effects.push(ScreenEffectState {
            kind,
            color,
            amount,
            elapsed_ms: 0,
            duration_ms: duration,
        });
        Ok(())
    }

    fn execute_sprite_image(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let id = self.string(self.operand(instruction, 0)?)?;
        let content = self.string(self.operand(instruction, 1)?)?;
        let x = self.float(self.operand(instruction, 2)?)?;
        let y = self.float(self.operand(instruction, 3)?)?;
        let z = self.integer(self.operand(instruction, 4)?)? as i32;
        let opacity = self.integer(self.operand(instruction, 5)?)?.clamp(0, 255) as u8;
        self.state.sprites.insert(
            id.clone(),
            SpriteState {
                id,
                kind: SpriteKind::Image,
                content,
                bounds: Rect {
                    x,
                    y,
                    width: 0.0,
                    height: 0.0,
                },
                color: Color::WHITE,
                font_size: 24.0,
                opacity,
                z,
                visible: true,
                scale: 1.0,
                rotation_degrees: 0.0,
                tint: Color::WHITE,
            },
        );
        Ok(())
    }

    fn execute_sprite_rect(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let id = self.string(self.operand(instruction, 0)?)?;
        let x = self.float(self.operand(instruction, 1)?)?;
        let y = self.float(self.operand(instruction, 2)?)?;
        let width = self.float(self.operand(instruction, 3)?)?;
        let height = self.float(self.operand(instruction, 4)?)?;
        let color =
            parse_color(&self.string(self.operand(instruction, 5)?)?).unwrap_or(Color::BLACK);
        let z = self.integer(self.operand(instruction, 6)?)? as i32;
        self.state.sprites.insert(
            id.clone(),
            SpriteState {
                id,
                kind: SpriteKind::Rectangle,
                content: String::new(),
                bounds: Rect {
                    x,
                    y,
                    width,
                    height,
                },
                color,
                font_size: 24.0,
                opacity: color.alpha,
                z,
                visible: true,
                scale: 1.0,
                rotation_degrees: 0.0,
                tint: Color::WHITE,
            },
        );
        Ok(())
    }

    fn execute_sprite_text(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let id = self.string(self.operand(instruction, 0)?)?;
        let content = self.string(self.operand(instruction, 1)?)?;
        let x = self.float(self.operand(instruction, 2)?)?;
        let y = self.float(self.operand(instruction, 3)?)?;
        let font_size = self.float(self.operand(instruction, 4)?)?.max(1.0);
        let z = self.integer(self.operand(instruction, 5)?)? as i32;
        self.state.sprites.insert(
            id.clone(),
            SpriteState {
                id,
                kind: SpriteKind::Text,
                content,
                bounds: Rect {
                    x,
                    y,
                    width: self.logical_size.width as f32 - x,
                    height: font_size * 1.5,
                },
                color: Color::WHITE,
                font_size,
                opacity: 255,
                z,
                visible: true,
                scale: 1.0,
                rotation_degrees: 0.0,
                tint: Color::WHITE,
            },
        );
        Ok(())
    }

    fn execute_choice(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        if instruction.operands.len() < 2 || !instruction.operands.len().is_multiple_of(2) {
            return Err(self.error_at_pc(VmErrorKind::InvalidOperands(
                "choice requires text/address pairs".to_owned(),
            )));
        }
        let mut options = Vec::new();
        for pair in instruction.operands.chunks_exact(2) {
            options.push(ChoiceOptionState {
                text: self.string(&pair[0])?,
                target: self.address(&pair[1])?,
            });
        }
        self.state.choice = Some(ChoiceState {
            options,
            focused: 0,
        });
        self.state.execution = ExecutionState::WaitingForChoice;
        Ok(())
    }

    fn execute_audio_play(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let bus = parse_audio_bus(&self.string(self.operand(instruction, 0)?)?)?;
        let id = self.string(self.operand(instruction, 1)?)?;
        let asset = self.string(self.operand(instruction, 2)?)?;
        let looping = self.boolean(self.operand(instruction, 3)?)?;
        let volume = normalize_volume(self.float(self.operand(instruction, 4)?)?);
        let fade_in_ms = self.integer(self.operand(instruction, 5)?)?.max(0) as u32;
        let key = audio_key(bus, &id);
        self.state.audio_tracks.insert(
            key,
            AudioTrackState {
                bus,
                id: id.clone(),
                asset: asset.clone(),
                looping,
                volume,
            },
        );
        self.pending_audio.push(AudioCommand::Play {
            bus,
            id,
            asset,
            looping,
            volume,
            fade_in_ms,
        });
        Ok(())
    }

    fn execute_audio_stop(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let bus = parse_audio_bus(&self.string(self.operand(instruction, 0)?)?)?;
        let id = match self.operand(instruction, 1)? {
            Operand::None => None,
            value => Some(self.string(value)?),
        };
        let fade_out_ms = self.integer(self.operand(instruction, 2)?)?.max(0) as u32;
        if let Some(id) = &id {
            self.state.audio_tracks.remove(&audio_key(bus, id));
        } else {
            self.state.audio_tracks.retain(|_, track| track.bus != bus);
        }
        self.pending_audio.push(AudioCommand::Stop {
            bus,
            id,
            fade_out_ms,
        });
        Ok(())
    }

    fn execute_volume(&mut self, instruction: &EncodedInstruction) -> Result<(), VmError> {
        let bus = parse_audio_bus(&self.string(self.operand(instruction, 0)?)?)?;
        let volume = normalize_volume(self.float(self.operand(instruction, 1)?)?);
        let fade_ms = self.integer(self.operand(instruction, 2)?)?.max(0) as u32;
        self.state.bus_volumes.insert(bus, volume);
        self.pending_audio.push(AudioCommand::SetBusVolume {
            bus,
            volume,
            fade_ms,
        });
        Ok(())
    }

    fn operand<'a>(
        &self,
        instruction: &'a EncodedInstruction,
        index: usize,
    ) -> Result<&'a Operand, VmError> {
        instruction.operands.get(index).ok_or_else(|| {
            self.error_at_pc(VmErrorKind::InvalidOperands(format!(
                "{:?} is missing operand {index}",
                instruction.op
            )))
        })
    }

    fn value(&self, operand: &Operand) -> Result<RuntimeValue, VmError> {
        match operand {
            Operand::Constant(index) => match self.constant(*index)? {
                Constant::String(value) => Ok(RuntimeValue::String(value.clone())),
                Constant::Integer(value) => Ok(RuntimeValue::Integer(*value)),
                Constant::Float(value) => Ok(RuntimeValue::Float(*value)),
            },
            Operand::Integer(value) => Ok(RuntimeValue::Integer(*value)),
            Operand::Float(value) => Ok(RuntimeValue::Float(f64::from(*value))),
            Operand::Boolean(value) => Ok(RuntimeValue::Boolean(*value)),
            Operand::IntRegister(name) => Ok(RuntimeValue::Integer(
                self.state.int_registers.get(name).copied().unwrap_or(0),
            )),
            Operand::StringRegister(name) => Ok(RuntimeValue::String(
                self.state
                    .string_registers
                    .get(name)
                    .cloned()
                    .unwrap_or_default(),
            )),
            Operand::Address(value) => Ok(RuntimeValue::Integer(i64::from(*value))),
            Operand::None => Ok(RuntimeValue::None),
        }
    }

    fn string(&self, operand: &Operand) -> Result<String, VmError> {
        Ok(match self.value(operand)? {
            RuntimeValue::String(value) => value,
            RuntimeValue::Integer(value) => value.to_string(),
            RuntimeValue::Float(value) => value.to_string(),
            RuntimeValue::Boolean(value) => value.to_string(),
            RuntimeValue::None => String::new(),
        })
    }

    fn integer(&self, operand: &Operand) -> Result<i64, VmError> {
        match self.value(operand)? {
            RuntimeValue::Integer(value) => Ok(value),
            RuntimeValue::Float(value) => Ok(value as i64),
            RuntimeValue::Boolean(value) => Ok(i64::from(value)),
            RuntimeValue::String(value) => value.parse().map_err(|_| {
                self.error_at_pc(VmErrorKind::TypeMismatch(format!(
                    "'{value}' is not an integer"
                )))
            }),
            RuntimeValue::None => Ok(0),
        }
    }

    fn float(&self, operand: &Operand) -> Result<f32, VmError> {
        match self.value(operand)? {
            RuntimeValue::Integer(value) => Ok(value as f32),
            RuntimeValue::Float(value) => Ok(value as f32),
            RuntimeValue::Boolean(value) => Ok(if value { 1.0 } else { 0.0 }),
            RuntimeValue::String(value) => value.parse().map_err(|_| {
                self.error_at_pc(VmErrorKind::TypeMismatch(format!(
                    "'{value}' is not a number"
                )))
            }),
            RuntimeValue::None => Ok(0.0),
        }
    }

    fn boolean(&self, operand: &Operand) -> Result<bool, VmError> {
        Ok(match self.value(operand)? {
            RuntimeValue::Boolean(value) => value,
            RuntimeValue::Integer(value) => value != 0,
            RuntimeValue::Float(value) => value != 0.0,
            RuntimeValue::String(value) => {
                !value.is_empty()
                    && !value.eq_ignore_ascii_case("false")
                    && !value.eq_ignore_ascii_case("off")
                    && value != "0"
            }
            RuntimeValue::None => false,
        })
    }

    fn address(&self, operand: &Operand) -> Result<u32, VmError> {
        let Operand::Address(address) = operand else {
            return Err(self.error_at_pc(VmErrorKind::TypeMismatch(
                "expected bytecode address".to_owned(),
            )));
        };
        Ok(*address)
    }

    fn int_register_name(&self, operand: &Operand) -> Result<String, VmError> {
        let Operand::IntRegister(name) = operand else {
            return Err(self.error_at_pc(VmErrorKind::TypeMismatch(
                "expected integer register".to_owned(),
            )));
        };
        Ok(name.clone())
    }

    fn string_register_name(&self, operand: &Operand) -> Result<String, VmError> {
        let Operand::StringRegister(name) = operand else {
            return Err(self.error_at_pc(VmErrorKind::TypeMismatch(
                "expected string register".to_owned(),
            )));
        };
        Ok(name.clone())
    }

    fn constant(&self, index: u32) -> Result<&Constant, VmError> {
        self.program
            .constants
            .get(index as usize)
            .ok_or_else(|| self.error_at_pc(VmErrorKind::MissingConstant(index)))
    }

    fn clear_text(&mut self) {
        let speed_ms = self.state.text.speed_ms;
        self.state.text = TextState {
            speed_ms,
            ..TextState::default()
        };
    }

    /// Projects narrative scene state into a canvas-only frame. Textbox,
    /// choice, menu, and host layout are represented exclusively in
    /// [`UiViewModel`] and rendered by the presentation package.
    fn scene_frame(&self) -> SceneFrame {
        let mut commands = Vec::new();
        let frame_size = self.frame_logical_size();
        let full = Rect {
            x: 0.0,
            y: 0.0,
            width: frame_size.width as f32,
            height: frame_size.height as f32,
        };
        if let Some(background) = &self.state.background {
            if let Some(color) = parse_color(&background.asset) {
                commands.push(DrawCommand::Rectangle {
                    id: "scene.background".to_owned(),
                    bounds: full,
                    color,
                    corner_radius: 0.0,
                    z: -10_000,
                    style: DrawStyle::default(),
                });
            } else {
                commands.push(DrawCommand::Sprite {
                    id: "scene.background".to_owned(),
                    asset: background.asset.clone(),
                    destination: full,
                    opacity: 255,
                    z: -10_000,
                    visible: true,
                    blend: BlendMode::Alpha,
                    mask: None,
                    scale: 1.0,
                    rotation_degrees: 0.0,
                    tint: Color::WHITE,
                    fit: SpriteFit::Cover,
                    style: DrawStyle::default(),
                });
            }
        }
        let scene_projection = scene_projection(self.logical_size, frame_size);
        for sprite in self.state.sprites.values() {
            match sprite.kind {
                SpriteKind::Image => {
                    let intrinsic = sprite.bounds.width <= 0.0 || sprite.bounds.height <= 0.0;
                    commands.push(DrawCommand::Sprite {
                        id: sprite.id.clone(),
                        asset: sprite.content.clone(),
                        destination: project_scene_rect(sprite.bounds, scene_projection),
                        opacity: sprite.opacity,
                        z: sprite.z,
                        visible: sprite.visible,
                        blend: BlendMode::Alpha,
                        mask: None,
                        scale: if intrinsic {
                            sprite.scale * scene_projection.scale
                        } else {
                            sprite.scale
                        },
                        rotation_degrees: sprite.rotation_degrees,
                        tint: sprite.tint,
                        fit: SpriteFit::Contain,
                        style: DrawStyle::default(),
                    });
                }
                SpriteKind::Rectangle if sprite.visible => {
                    commands.push(DrawCommand::Rectangle {
                        id: sprite.id.clone(),
                        bounds: project_scene_rect(sprite.bounds, scene_projection),
                        color: sprite.color,
                        corner_radius: 0.0,
                        z: sprite.z,
                        style: DrawStyle::default(),
                    });
                }
                SpriteKind::Text if sprite.visible => commands.push(DrawCommand::Text {
                    id: sprite.id.clone(),
                    text: sprite.content.clone(),
                    speaker: None,
                    bounds: project_scene_rect(sprite.bounds, scene_projection),
                    color: sprite.color,
                    font_size: sprite.font_size * scene_projection.scale,
                    z: sprite.z,
                    style: DrawStyle::default(),
                }),
                _ => {}
            }
        }
        let effects = self
            .state
            .effects
            .iter()
            .map(|effect| {
                let progress = if effect.duration_ms == 0 {
                    1.0
                } else {
                    (effect.elapsed_ms as f32 / effect.duration_ms as f32).clamp(0.0, 1.0)
                };
                match effect.kind {
                    ScreenEffectKind::Tint => ScreenEffect::Tint {
                        color: effect.color,
                        opacity: effect.amount.clamp(0.0, 255.0) as u8,
                        progress,
                    },
                    ScreenEffectKind::Flash => ScreenEffect::Flash {
                        color: effect.color,
                        opacity: effect.amount.clamp(0.0, 255.0) as u8,
                        progress,
                    },
                    ScreenEffectKind::Shake => ScreenEffect::Shake {
                        amplitude: effect.amount,
                        progress,
                    },
                }
            })
            .collect();
        let transition = self.state.transition.as_ref().map(|transition| {
            let progress = if transition.duration_ms == 0 {
                1.0
            } else {
                (transition.elapsed_ms as f32 / transition.duration_ms as f32).clamp(0.0, 1.0)
            };
            TransitionFrame {
                kind: transition.kind.clone(),
                progress,
            }
        });
        let mut frame = SceneFrame {
            frame_number: self.state.frame_number,
            logical_size: frame_size,
            viewport: self.state.ui.viewport,
            clear_color: Color::BLACK,
            commands,
            transition,
            effects,
        };
        frame.sort_commands();
        frame
    }

    fn build_view_model(&self) -> UiViewModel {
        let route = UiRoute::parse(&self.state.ui.route);
        let timed_hold_remaining_ms = match &self.state.execution {
            ExecutionState::WaitingForDelay { remaining_ms } => Some(*remaining_ms),
            _ => None,
        };
        let dialogue_pages = self.subtitle_pages();
        let dialogue_page_index = self
            .state
            .text
            .page_index
            .min(dialogue_pages.len().saturating_sub(1));
        let dialogue_page = dialogue_pages
            .get(dialogue_page_index)
            .cloned()
            .unwrap_or_default();
        let dialogue_total = grapheme_count(&dialogue_page);
        let dialogue = (!self.state.text.full_text.is_empty()).then(|| DialogueView {
            speaker: self.state.text.speaker.clone(),
            full_text: self.state.text.full_text.clone(),
            full_page_text: dialogue_page.clone(),
            page_number: u32::try_from(dialogue_page_index.saturating_add(1)).unwrap_or(u32::MAX),
            page_count: u32::try_from(dialogue_pages.len()).unwrap_or(u32::MAX),
            page_id: self.current_page_id().unwrap_or_default(),
            columns: u16::try_from(self.state.text.page_columns).unwrap_or(u16::MAX),
            text: grapheme_prefix(&dialogue_page, self.state.text.visible_graphemes),
            complete: self.state.text.visible_graphemes >= dialogue_total,
            awaiting_advance: matches!(
                self.state.execution,
                ExecutionState::WaitingForAdvance { .. }
            ),
        });
        let interlude = (self.state.ui.route == "interlude").then_some(InterludeView {
            first_visit: self.state.ui.interlude_first_visit,
        });
        let choices = self
            .state
            .choice
            .as_ref()
            .map(|choice| {
                choice
                    .options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| ChoiceView {
                        id: format!("choice:{index}"),
                        label: option.text.clone(),
                        selected: index == choice.focused,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let backlog_total = u32::try_from(self.state.backlog.len()).unwrap_or(u32::MAX);
        let backlog_start = self.backlog_window_start();
        let backlog = if matches!(route, UiRoute::Backlog) {
            self.state
                .backlog
                .iter()
                .enumerate()
                .skip(backlog_start)
                .take(BACKLOG_WINDOW_SIZE)
                .map(|(index, entry)| BacklogEntryView {
                    id: entry.id.clone(),
                    resume_id: entry.id.clone(),
                    speaker: entry.speaker.clone(),
                    text: entry.text.clone(),
                    locale: entry.locale.clone(),
                    timestamp_ms: entry.timestamp_ms,
                    selected: index == self.state.backlog_focused,
                })
                .collect()
        } else {
            Vec::new()
        };
        let chapters = self
            .state
            .chapters
            .values()
            .map(|chapter| ChapterView {
                id: chapter.id.clone(),
                title: chapter.title.clone(),
                description: chapter.description.clone(),
                thumbnail: chapter.thumbnail.clone(),
                script: chapter.script.clone(),
                unlocked: chapter.unlocked,
                progress: chapter.progress,
            })
            .collect();
        let gallery = self
            .state
            .unlocked_cgs
            .iter()
            .map(|id| GalleryItemView {
                id: id.clone(),
                unlocked: true,
                selected: self.state.ui.gallery_viewer.as_deref() == Some(id.as_str()),
            })
            .collect();
        UiViewModel {
            schema_version: UI_VIEW_MODEL_SCHEMA,
            route: route.clone(),
            route_stack: self
                .state
                .ui
                .route_stack
                .iter()
                .map(|route| UiRoute::parse(route))
                .collect(),
            game: GameView {
                id: self.program.game_id.clone(),
                locale: self.state.locale.clone(),
            },
            dialogue,
            timed_hold_remaining_ms,
            choices,
            actions: self.presentation_actions(&route),
            settings: self.state.settings.clone(),
            backlog,
            backlog_total,
            backlog_start: u32::try_from(backlog_start).unwrap_or(u32::MAX),
            chapters,
            gallery,
            gallery_viewer: self.state.ui.gallery_viewer.clone(),
            interlude,
            confirmation: self.state.ui.confirmation.as_ref().map(|confirmation| {
                ConfirmationView {
                    action: confirmation.action.clone(),
                    resume_id: confirmation.resume_id.clone(),
                }
            }),
            scroll_offsets: self.state.ui.scroll_offsets.clone(),
            auto_mode: self.state.auto_mode,
            skip_mode: self.state.skip_mode,
            reduced_motion: self.state.settings.reduced_motion,
        }
    }

    fn presentation_actions(&self, route: &UiRoute) -> Vec<ActionView> {
        let action = |id: &str, active: bool| ActionView {
            id: id.to_owned(),
            enabled: true,
            active,
        };
        match route {
            UiRoute::Setup => Vec::new(),
            UiRoute::Title => vec![
                action("route:load", false),
                action("route:settings", false),
                action("route:gallery", false),
            ],
            // A demo endpoint is authored through explicit choices. It has
            // no implicit menu action: the player can only replay the
            // available record or return to its title.
            UiRoute::DemoEnd => Vec::new(),
            UiRoute::Dialogue => vec![
                action("chrome.menu", false),
                action("chrome.backlog", false),
                action("menu.auto", self.state.auto_mode == AutoMode::On),
                action("menu.skip", self.state.skip_mode != SkipMode::Off),
            ],
            UiRoute::Pause => vec![
                action("menu.save", false),
                action("menu.load", false),
                action("menu.backlog", false),
                action("menu.gallery", false),
                action("menu.settings", false),
                action("menu.auto", self.state.auto_mode == AutoMode::On),
                action("menu.skip", self.state.skip_mode != SkipMode::Off),
                action("menu.reset", false),
                action("menu.quit", false),
                action("dismiss", false),
            ],
            UiRoute::Save => (1..=10)
                .map(|slot| action(&format!("save.slot.{slot}"), false))
                .chain(std::iter::once(action("dismiss", false)))
                .collect(),
            // The host-maintained automatic checkpoint is deliberately not a
            // player-facing record. Archive UI contains only deliberate,
            // named manual saves.
            UiRoute::Load => (1..=10)
                .map(|slot| action(&format!("load.slot.{slot}"), false))
                .chain(std::iter::once(action("dismiss", false)))
                .collect(),
            UiRoute::Settings => vec![action("dismiss", false)],
            UiRoute::Backlog => self
                .state
                .backlog
                .iter()
                .skip(self.backlog_window_start())
                .take(BACKLOG_WINDOW_SIZE)
                .map(|entry| action(&format!("backlog:{}", entry.id), false))
                .chain(std::iter::once(action("dismiss", false)))
                .collect(),
            // Chapter selection begins with a line of story text before its
            // catalogue opens. That reading surface has the same rmenu
            // affordance as ordinary dialogue; otherwise a right click there
            // is either ignored or accidentally advances the line.
            UiRoute::ChapterSelect => vec![
                action("chrome.menu", false),
                action("chrome.backlog", false),
                action("menu.auto", self.state.auto_mode == AutoMode::On),
                action("menu.skip", self.state.skip_mode != SkipMode::Off),
            ]
            .into_iter()
            .chain(self.state.chapters.values().map(|chapter| ActionView {
                id: format!("chapter:{}", chapter.id),
                enabled: chapter.unlocked,
                active: false,
            }))
            .chain(std::iter::once(action("dismiss", false)))
            .collect(),
            UiRoute::Gallery => self
                .state
                .unlocked_cgs
                .iter()
                .map(|id| action(&format!("gallery:{id}"), false))
                .chain(
                    self.state
                        .ui
                        .gallery_viewer
                        .as_ref()
                        .into_iter()
                        .flat_map(|_| {
                            [
                                action("gallery.previous", false),
                                action("gallery.next", false),
                                action("gallery.close", false),
                            ]
                        }),
                )
                .chain(std::iter::once(action("dismiss", false)))
                .collect(),
            UiRoute::Confirm => vec![
                action("confirm.accept", false),
                action("confirm.cancel", false),
                action("dismiss", false),
            ],
            // `day_card` is intentionally a project-specific route, rather
            // than a layout baked into Core. It still exposes the two quiet
            // reading affordances so keyboard, gamepad, and right-click are
            // consistent while the chapter waits for its one semantic choice.
            UiRoute::Custom(route) if route == "day_card" => vec![
                action("chrome.menu", false),
                action("chrome.backlog", false),
            ],
            // Interludes share the quiet controls with the reader. The
            // semantic advance action is intentionally distinct from
            // dialogue.advance: it releases a held silence rather than a
            // subtitle page.
            UiRoute::Custom(route) if route == "interlude" => vec![
                action("chrome.menu", false),
                action("chrome.backlog", false),
                action("menu.auto", self.state.auto_mode == AutoMode::On),
                action("menu.skip", self.state.skip_mode != SkipMode::Off),
                action("interlude.advance", false),
            ],
            // A statement is intentionally not a button disguised as a
            // cinematic screen. It exposes only the same escape hatches as
            // reading (RMenu, backlog, auto/skip) and lets its authored wait
            // determine when the next subtitle may begin.
            UiRoute::Custom(route) if route == "statement" => vec![
                action("chrome.menu", false),
                action("chrome.backlog", false),
                action("menu.auto", self.state.auto_mode == AutoMode::On),
                action("menu.skip", self.state.skip_mode != SkipMode::Off),
            ],
            UiRoute::Custom(_) => vec![action("dismiss", false)],
        }
    }

    fn error_at_pc(&self, kind: VmErrorKind) -> VmError {
        let executed_pc = self.state.pc.saturating_sub(1);
        let source = self
            .program
            .source_map
            .get(executed_pc as usize)
            .map(|location| format!("{}:{}:{}", location.source, location.line, location.column))
            .unwrap_or_else(|| "<unknown>".to_owned());
        VmError::Runtime {
            pc: executed_pc,
            location: source,
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SceneProjection {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

fn scene_projection(authored: LogicalSize, target: LogicalSize) -> SceneProjection {
    let authored_width = authored.width.max(1) as f32;
    let authored_height = authored.height.max(1) as f32;
    let target_width = target.width.max(1) as f32;
    let target_height = target.height.max(1) as f32;
    // Scene artwork is a photographic layer. Cover keeps its composition
    // intact while allowing the DOM presentation to occupy the actual viewport.
    let scale = (target_width / authored_width)
        .max(target_height / authored_height)
        .max(f32::EPSILON);
    SceneProjection {
        scale,
        offset_x: (target_width - authored_width * scale) * 0.5,
        offset_y: (target_height - authored_height * scale) * 0.5,
    }
}

fn project_scene_rect(bounds: Rect, projection: SceneProjection) -> Rect {
    Rect {
        x: projection.offset_x + bounds.x * projection.scale,
        y: projection.offset_y + bounds.y * projection.scale,
        width: bounds.width * projection.scale,
        height: bounds.height * projection.scale,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum RuntimeValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    None,
}

fn compare(left: &RuntimeValue, comparator: &str, right: &RuntimeValue) -> bool {
    if comparator == "truthy" {
        return match left {
            RuntimeValue::String(value) => !value.is_empty(),
            RuntimeValue::Integer(value) => *value != 0,
            RuntimeValue::Float(value) => *value != 0.0,
            RuntimeValue::Boolean(value) => *value,
            RuntimeValue::None => false,
        };
    }
    let numeric = match (left, right) {
        (RuntimeValue::Integer(left), RuntimeValue::Integer(right)) => {
            Some((*left as f64, *right as f64))
        }
        (RuntimeValue::Integer(left), RuntimeValue::Float(right)) => Some((*left as f64, *right)),
        (RuntimeValue::Float(left), RuntimeValue::Integer(right)) => Some((*left, *right as f64)),
        (RuntimeValue::Float(left), RuntimeValue::Float(right)) => Some((*left, *right)),
        _ => None,
    };
    if let Some((left, right)) = numeric {
        return match comparator {
            "=" | "==" => left == right,
            "!=" => left != right,
            ">" => left > right,
            ">=" => left >= right,
            "<" => left < right,
            "<=" => left <= right,
            _ => false,
        };
    }
    let left = runtime_string(left);
    let right = runtime_string(right);
    match comparator {
        "=" | "==" => left == right,
        "!=" => left != right,
        ">" => left > right,
        ">=" => left >= right,
        "<" => left < right,
        "<=" => left <= right,
        _ => false,
    }
}

fn runtime_string(value: &RuntimeValue) -> String {
    match value {
        RuntimeValue::String(value) => value.clone(),
        RuntimeValue::Integer(value) => value.to_string(),
        RuntimeValue::Float(value) => value.to_string(),
        RuntimeValue::Boolean(value) => value.to_string(),
        RuntimeValue::None => String::new(),
    }
}

fn ease(progress: f32, name: &str) -> f32 {
    match name.to_ascii_lowercase().as_str() {
        "ease_in" | "in" => progress * progress,
        "ease_out" | "out" => 1.0 - (1.0 - progress) * (1.0 - progress),
        "ease_in_out" | "in_out" => {
            if progress < 0.5 {
                2.0 * progress * progress
            } else {
                1.0 - (-2.0 * progress + 2.0).powi(2) / 2.0
            }
        }
        _ => progress,
    }
}

fn parse_skip_mode(value: &str) -> SkipMode {
    match value.to_ascii_lowercase().as_str() {
        "all" | "on" | "true" => SkipMode::All,
        "read" | "read_only" => SkipMode::Read,
        _ => SkipMode::Off,
    }
}

fn parse_effect_kind(value: &str) -> ScreenEffectKind {
    match value.to_ascii_lowercase().as_str() {
        "flash" => ScreenEffectKind::Flash,
        "shake" | "quake" => ScreenEffectKind::Shake,
        _ => ScreenEffectKind::Tint,
    }
}

fn normalize_screen_name(value: &str) -> &str {
    match value.to_ascii_lowercase().as_str() {
        "menu" | "pause" | "rmenu" => "pause",
        "save" => "save",
        "load" => "load",
        "settings" | "config" => "settings",
        "backlog" | "log" => "backlog",
        "title" => "title",
        "chapter" | "chapter_select" => "chapter_select",
        "gallery" | "cg" => "gallery",
        "interlude" => "interlude",
        "statement" => "statement",
        _ => value,
    }
}

fn presentation_manual_save_slot(id: &str) -> Option<u32> {
    let slot = id.strip_prefix("save.slot.")?.parse::<u32>().ok()?;
    (1..=10).contains(&slot).then_some(slot)
}

fn presentation_load_slot(id: &str) -> Option<u32> {
    let slot = id.strip_prefix("load.slot.")?.parse::<u32>().ok()?;
    (1..=10).contains(&slot).then_some(slot)
}

fn is_standard_route(value: &str) -> bool {
    matches!(
        value,
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
            | "confirm"
            | "day_card"
            | "interlude"
            | "statement"
    )
}

fn is_atomic_story_route(route: &str) -> bool {
    matches!(route, "interlude" | "statement")
}

fn is_setting_name(value: &str) -> bool {
    matches!(
        value,
        "text_speed_ms"
            | "auto_delay_ms"
            | "bgm_volume"
            | "sound_effect_volume"
            | "voice_volume"
            | "text_scale"
            | "text_opacity"
    )
}

fn parse_audio_bus(value: &str) -> Result<AudioBus, VmError> {
    match value {
        "bgm" => Ok(AudioBus::Bgm),
        "sound_effect" | "se" => Ok(AudioBus::SoundEffect),
        "voice" => Ok(AudioBus::Voice),
        other => Err(VmError::InvalidAudioBus(other.to_owned())),
    }
}

fn normalize_volume(value: f32) -> f32 {
    if value > 1.0 {
        (value / 100.0).clamp(0.0, 1.0)
    } else {
        value.clamp(0.0, 1.0)
    }
}

fn audio_key(bus: AudioBus, id: &str) -> String {
    format!("{bus:?}:{id}")
}

fn parse_transition(value: &str) -> TransitionKind {
    match value.to_ascii_lowercase().as_str() {
        "crossfade" | "cross_fade" => TransitionKind::CrossFade,
        "wipeleft" | "wipe_left" => TransitionKind::WipeLeft,
        "wiperight" | "wipe_right" => TransitionKind::WipeRight,
        "fade" => TransitionKind::Fade,
        mask => TransitionKind::Mask(mask.to_owned()),
    }
}

#[must_use]
pub fn parse_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    match hex.len() {
        6 => Some(Color {
            red: u8::from_str_radix(&hex[0..2], 16).ok()?,
            green: u8::from_str_radix(&hex[2..4], 16).ok()?,
            blue: u8::from_str_radix(&hex[4..6], 16).ok()?,
            alpha: 255,
        }),
        8 => Some(Color {
            red: u8::from_str_radix(&hex[0..2], 16).ok()?,
            green: u8::from_str_radix(&hex[2..4], 16).ok()?,
            blue: u8::from_str_radix(&hex[4..6], 16).ok()?,
            alpha: u8::from_str_radix(&hex[6..8], 16).ok()?,
        }),
        _ => None,
    }
}

/// The reading surface is a two-line subtitle grid, not an unconstrained DOM
/// paragraph. One display cell is an ASCII half-em in the bundled monospaced
/// face; CJK graphemes occupy two cells. The same conservative grid is used
/// by Web and Native, and its value is captured in `TextState` when a line
/// begins so a save/history page has a stable boundary.
fn subtitle_columns(viewport: UiViewport, text_scale: f32) -> usize {
    let width = viewport.content_width();
    let scale = text_scale.clamp(0.85, 1.35);
    // Keep this in lockstep with the reading-band grid in app.css. The band
    // reserves responsive side margins, a 44px advance target, and its gap;
    // using the raw viewport here would make a CJK line fit Core's grid but
    // be clipped by the actual black band at mid-width screens.
    let side_inset = if width <= 639.0 {
        19.2 * scale // 1.2rem mobile inset
    } else {
        (width * 0.10).clamp(24.0 * scale, 184.0 * scale)
    };
    let advance_gap = (width * 0.016).clamp(8.8 * scale, 18.4 * scale);
    let cell_width = 10.0 * scale;
    let usable_width = (width - side_inset * 2.0 - 44.0 - advance_gap).max(cell_width);
    (usable_width / cell_width).floor().clamp(1.0, 112.0) as usize
}

#[derive(Debug, Error)]
pub enum VmError {
    #[error("invalid compiled program: {0}")]
    InvalidProgram(String),
    #[error("logical resolution must be non-zero")]
    InvalidLogicalSize,
    #[error("input sequence must increase (previous {previous}, received {received})")]
    NonMonotonicInput { previous: u64, received: u64 },
    #[error("UI viewport is invalid")]
    InvalidViewport,
    #[error("UI scroll delta is invalid")]
    InvalidScrollDelta,
    #[error("invalid UI intent: {0}")]
    InvalidUiIntent(String),
    #[error("unsupported VM snapshot schema {0}")]
    UnsupportedSnapshot(u32),
    #[error("snapshot belongs to '{actual}', expected '{expected}'")]
    WrongGame { expected: String, actual: String },
    #[error("invalid snapshot program counter {0}")]
    InvalidSnapshotPc(u32),
    #[error("unknown audio bus '{0}'")]
    InvalidAudioBus(String),
    #[error("VM error at pc {pc} ({location}): {kind}")]
    Runtime {
        pc: u32,
        location: String,
        kind: VmErrorKind,
    },
}

#[derive(Debug, Error)]
pub enum VmErrorKind {
    #[error("program counter is outside the instruction table")]
    ProgramCounterOutOfRange,
    #[error("instruction budget exceeded; probable infinite loop")]
    InstructionBudgetExceeded,
    #[error("missing constant {0}")]
    MissingConstant(u32),
    #[error("invalid operands: {0}")]
    InvalidOperands(String),
    #[error("type mismatch: {0}")]
    TypeMismatch(String),
    #[error("VM is waiting for a choice but choice state is absent")]
    MissingChoiceState,
    #[error("return executed without a matching call")]
    ReturnWithoutCaller,
    #[error("call stack exceeded the deterministic 1024-frame limit")]
    CallDepthExceeded,
}

#[cfg(test)]
mod tests {
    use crate::compiler::{CompileInput, SourceUnit, compile};

    use super::*;

    const SIZE: LogicalSize = LogicalSize {
        width: 1280,
        height: 720,
    };

    fn vm(source: &str) -> Vm {
        let compiled = compile(CompileInput {
            game_id: "jp.example.test".to_owned(),
            entry: "main.aria".to_owned(),
            sources: vec![SourceUnit {
                logical_path: "main.aria".to_owned(),
                source: source.to_owned(),
            }],
        });
        assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
        Vm::new(compiled.program.expect("compiled program"), SIZE).expect("valid VM")
    }

    #[test]
    fn choice_is_exposed_as_semantic_view_data_and_selects_from_an_intent() {
        let mut vm = vm(
            "aria;\nentry start;\nscene start { choice { \"海\" => sea; \"空\" => sky; } }\nscene sea { end; }\nscene sky { end; }\n",
        );
        let first = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert_eq!(first.view.choices[0].id, "choice:0");
        assert_eq!(first.view.choices[0].label, "海");
        assert!(first.scene.commands.is_empty());

        let mut input = InputSnapshot::idle(2, 16);
        input.intents.push(UiIntent::Activate {
            id: "choice:0".to_owned(),
        });
        let selected = vm.step(&input).unwrap();
        assert!(selected.halted);
    }

    #[test]
    fn duplicate_choice_activation_is_ignored_after_the_first_selection() {
        let mut vm = vm("aria;\nentry start;\n\
             scene start { choice { \"海\" => sea; } }\n\
             scene sea { screen dialogue; narrate \"進む。\"; await advance; end; }\n");
        let _choice = vm.step(&InputSnapshot::idle(1, 16)).unwrap();

        // A touch/click and a key can be placed in one host frame while the
        // selection's next surface is still committing.  They must select
        // once, never crash the VM because the first action cleared state.
        let mut input = InputSnapshot::idle(2, 16);
        input.intents.push(UiIntent::Activate {
            id: "choice:0".to_owned(),
        });
        input.intents.push(UiIntent::Activate {
            id: "choice:0".to_owned(),
        });
        let selected = vm.step(&input).unwrap();

        assert_eq!(selected.view.route, UiRoute::Dialogue);
        assert!(selected.view.choices.is_empty());
        assert_eq!(
            selected
                .view
                .dialogue
                .as_ref()
                .map(|line| line.full_text.as_str()),
            Some("進む。")
        );
        assert_eq!(vm.state.narrative_trace.len(), 1);
    }

    #[test]
    fn day_card_is_a_saveable_reading_checkpoint() {
        let mut runtime = vm("aria;\n\
             entry start;\n\
             scene start { screen day_card; choice { \"DAY 1\\n9月21日・横浜\\n知らない街へ向かう朝。\" => chapter; } }\n\
             scene chapter { screen dialogue; narrate \"海へ向かう。\"; await advance; end; }\n");
        let card = runtime.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert_eq!(card.view.route, UiRoute::Custom("day_card".to_owned()));
        assert!(
            card.view
                .actions
                .iter()
                .any(|action| action.id == "chrome.menu")
        );
        assert_eq!(card.view.choices[0].id, "choice:0");

        // A save made while the day is held must restore that same pause, not
        // skip straight into the chapter or fall back to dialogue.
        let snapshot = runtime.snapshot();
        let mut restored = Vm::new(runtime.program.clone(), SIZE).unwrap();
        restored.restore(snapshot).unwrap();
        let restored_card = restored.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert_eq!(
            restored_card.view.route,
            UiRoute::Custom("day_card".to_owned())
        );
        assert_eq!(
            restored_card.view.choices[0].label,
            "DAY 1\n9月21日・横浜\n知らない街へ向かう朝。"
        );

        let mut open_menu = InputSnapshot::idle(2, 16);
        open_menu.intents.push(UiIntent::Activate {
            id: "chrome.menu".to_owned(),
        });
        assert_eq!(
            restored.step(&open_menu).unwrap().view.route,
            UiRoute::Pause
        );

        let mut close_menu = InputSnapshot::idle(3, 16);
        close_menu.intents.push(UiIntent::Dismiss);
        assert_eq!(
            restored.step(&close_menu).unwrap().view.route,
            UiRoute::Custom("day_card".to_owned())
        );

        let mut begin = InputSnapshot::idle(4, 16);
        begin.intents.push(UiIntent::Activate {
            id: "choice:0".to_owned(),
        });
        let chapter = restored.step(&begin).unwrap();
        assert_eq!(chapter.view.route, UiRoute::Dialogue);
        assert_eq!(
            chapter.view.dialogue.expect("chapter subtitle").full_text,
            "海へ向かう。"
        );
    }

    #[test]
    fn a_dialogue_hold_exposes_its_exact_wake_deadline_without_a_render_clock() {
        let mut runtime = vm("aria;\n\
             entry start;\n\
             scene start {\n\
               screen dialogue;\n\
               narrate \"前の文。\";\n\
               await advance;\n\
               clear dialogue;\n\
               wait 170ms;\n\
               narrate \"次の文。\";\n\
               await advance;\n\
               end;\n\
             }\n");

        runtime.step(&InputSnapshot::idle(1, 0)).unwrap();
        let opening = runtime.step(&InputSnapshot::idle(2, 1_000)).unwrap();
        assert!(opening.view.dialogue.expect("opening prose").complete);

        let mut advance = InputSnapshot::idle(3, 16);
        advance.intents.push(UiIntent::Activate {
            id: "dialogue.advance".to_owned(),
        });
        let hold = runtime.step(&advance).unwrap();
        assert_eq!(hold.view.schema_version, UI_VIEW_MODEL_SCHEMA);
        assert_eq!(hold.view.timed_hold_remaining_ms, Some(170));
        assert!(hold.view.dialogue.is_none());

        let resumed = runtime.step(&InputSnapshot::idle(4, 170)).unwrap();
        assert_eq!(resumed.view.timed_hold_remaining_ms, None);
        assert_eq!(
            resumed.view.dialogue.expect("following prose").full_text,
            "次の文。"
        );
    }

    #[test]
    fn interlude_is_logged_saveable_and_can_be_released_early() {
        let mut runtime = vm("aria;\n\
             entry start;\n\
             state mut interlude_seen: Bool = false;\n\
             scene start {\n\
               screen interlude;\n\
               clear dialogue;\n\
               narrate \"窓の外の季節は、誰かの許しを待たずに進む。\";\n\
               if interlude_seen { wait 220ms; } else { interlude_seen = true; wait 1200ms; }\n\
               screen dialogue;\n\
               narrate \"次の文章。\";\n\
               await advance;\n\
               end;\n\
             }\n");

        let beat = runtime.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert_eq!(beat.view.route, UiRoute::Custom("interlude".to_owned()));
        let dialogue = beat.view.dialogue.expect("interlude text");
        assert!(dialogue.complete);
        assert_eq!(dialogue.text, dialogue.full_page_text);
        assert_eq!(runtime.backlog().len(), 1);
        assert!(runtime.snapshot().read_texts.contains("text-0"));
        assert!(
            beat.view
                .actions
                .iter()
                .any(|action| action.id == "interlude.advance")
        );

        // A snapshot made inside the quiet hold restores the same semantic
        // route rather than collapsing it into an ordinary dialogue page.
        let snapshot = runtime.snapshot();
        let mut restored = Vm::new(runtime.program.clone(), SIZE).unwrap();
        restored.restore(snapshot).unwrap();
        let restored_beat = restored.step(&InputSnapshot::idle(2, 16)).unwrap();
        assert_eq!(
            restored_beat.view.route,
            UiRoute::Custom("interlude".to_owned())
        );
        assert_eq!(restored.backlog().len(), 1);

        let mut advance = InputSnapshot::idle(3, 16);
        advance.intents.push(UiIntent::Activate {
            id: "interlude.advance".to_owned(),
        });
        let prose = restored.step(&advance).unwrap();
        assert_eq!(prose.view.route, UiRoute::Dialogue);
        assert_eq!(
            prose.view.dialogue.expect("following subtitle").full_text,
            "次の文章。"
        );
    }

    #[test]
    fn statement_is_atomic_saveable_and_ignores_ordinary_advance_input() {
        let mut runtime = vm("aria;\n\
             entry start;\n\
             scene start {\n\
               screen statement;\n\
               narrate \"白い。\";\n\
               wait 1600ms;\n\
               screen dialogue;\n\
               clear dialogue;\n\
               narrate \"次の文章。\";\n\
               await advance;\n\
               end;\n\
             }\n");

        let opening = runtime.step(&InputSnapshot::idle(1, 0)).unwrap();
        assert_eq!(opening.view.route, UiRoute::Custom("statement".to_owned()));
        let dialogue = opening.view.dialogue.expect("statement text");
        assert!(dialogue.complete);
        assert_eq!(dialogue.full_page_text, "白い。");
        assert_eq!(opening.view.timed_hold_remaining_ms, Some(1600));
        assert_eq!(runtime.backlog().len(), 1);
        assert!(
            !opening
                .view
                .actions
                .iter()
                .any(|action| action.id == "interlude.advance")
        );

        // The normal reading key never turns a statement into a disguised
        // button. Its duration belongs to the script, not the player.
        let held = runtime
            .step(&InputSnapshot::pressed(2, 40, InputAction::Advance))
            .unwrap();
        assert_eq!(held.view.route, UiRoute::Custom("statement".to_owned()));
        assert_eq!(held.view.timed_hold_remaining_ms, Some(1560));

        let snapshot = runtime.snapshot();
        let mut restored = Vm::new(runtime.program.clone(), SIZE).unwrap();
        restored.restore(snapshot).unwrap();
        let restored_hold = restored.step(&InputSnapshot::idle(3, 100)).unwrap();
        assert_eq!(
            restored_hold.view.route,
            UiRoute::Custom("statement".to_owned())
        );
        assert_eq!(restored_hold.view.timed_hold_remaining_ms, Some(1460));

        let prose = restored.step(&InputSnapshot::idle(4, 1460)).unwrap();
        assert_eq!(prose.view.route, UiRoute::Dialogue);
        assert_eq!(
            prose.view.dialogue.expect("following subtitle").full_text,
            "次の文章。"
        );
    }

    #[test]
    fn stored_boolean_branches_follow_true_false_and_literal_comparisons() {
        let mut runtime = vm("aria;\n\
             entry start;\n\
             state mut gate: Bool = false;\n\
             scene start {\n\
               if gate { narrate \"wrong initial branch\"; await advance; } else { gate = true; }\n\
               if gate {\n\
                 if gate == true { narrate \"opened\"; await advance; } else { narrate \"wrong equality branch\"; await advance; }\n\
               } else { narrate \"wrong stored branch\"; await advance; }\n\
               end;\n\
             }\n");

        let output = runtime.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert_eq!(
            output
                .view
                .dialogue
                .expect("boolean branch prose")
                .full_text,
            "opened"
        );
    }

    #[test]
    fn interlude_clears_before_the_next_surface_and_replays_past_a_day_card() {
        let mut runtime = vm("aria;\n\
             entry start;\n\
             scene start {\n\
               screen interlude;\n\
               clear dialogue;\n\
               narrate \"断章。\";\n\
               wait 1200ms;\n\
               screen day_card;\n\
               choice { \"BEGIN\" => story; }\n\
             }\n\
             scene story {\n\
               screen dialogue;\n\
               wait 1ms;\n\
               narrate \"本文。\";\n\
               await advance;\n\
               end;\n\
             }\n");

        let opening = runtime.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert_eq!(opening.view.route, UiRoute::Custom("interlude".to_owned()));
        assert_eq!(runtime.backlog()[0].text, "断章。");

        let mut release = InputSnapshot::idle(2, 16);
        release.intents.push(UiIntent::Activate {
            id: "interlude.advance".to_owned(),
        });
        let card = runtime.step(&release).unwrap();
        assert_eq!(card.view.route, UiRoute::Custom("day_card".to_owned()));
        assert!(card.view.dialogue.is_none());

        let mut begin = InputSnapshot::idle(3, 16);
        begin.intents.push(UiIntent::Activate {
            id: "choice:0".to_owned(),
        });
        let pending_prose = runtime.step(&begin).unwrap();
        assert_eq!(pending_prose.view.route, UiRoute::Dialogue);
        assert!(pending_prose.view.dialogue.is_none());

        let prose = runtime.step(&InputSnapshot::idle(4, 1)).unwrap();
        assert_eq!(prose.view.dialogue.expect("prose text").full_text, "本文。");

        let mut reveal = InputSnapshot::idle(5, 16);
        reveal.intents.push(UiIntent::Activate {
            id: "dialogue.advance".to_owned(),
        });
        runtime.step(&reveal).unwrap();
        let prose_id = runtime
            .backlog()
            .last()
            .expect("completed prose backlog row")
            .id
            .clone();
        assert_eq!(runtime.backlog().len(), 2);

        let mut open_backlog = InputSnapshot::idle(6, 16);
        open_backlog.intents.push(UiIntent::Activate {
            id: "chrome.backlog".to_owned(),
        });
        runtime.step(&open_backlog).unwrap();
        let mut choose_prose = InputSnapshot::idle(7, 16);
        choose_prose.intents.push(UiIntent::Activate {
            id: format!("backlog:{prose_id}"),
        });
        runtime.step(&choose_prose).unwrap();
        let mut accept = InputSnapshot::idle(8, 16);
        accept.intents.push(UiIntent::Activate {
            id: "confirm.accept".to_owned(),
        });
        let resumed = runtime.step(&accept).unwrap();
        let dialogue = resumed.view.dialogue.expect("resumed prose");
        assert_eq!(resumed.view.route, UiRoute::Dialogue);
        assert_eq!(dialogue.page_id, prose_id);
        assert_eq!(dialogue.text, dialogue.full_page_text);
    }

    #[test]
    fn viewport_drives_scene_canvas_but_is_not_saved_as_ui_layout_state() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.test"), SIZE).unwrap();
        let output = vm
            .step(&InputSnapshot::idle(1, 16).with_viewport(UiViewport {
                width: 390,
                height: 844,
                scale_factor: 1.0,
                ..UiViewport::default()
            }))
            .unwrap();
        assert_eq!(output.scene.viewport.width, 390);
        assert_eq!(
            output.scene.logical_size,
            LogicalSize {
                width: 390,
                height: 844,
            }
        );
        let snapshot = serde_json::to_value(vm.snapshot()).unwrap();
        let ui = snapshot
            .get("ui")
            .and_then(serde_json::Value::as_object)
            .unwrap();
        assert!(!ui.contains_key("viewport"));
        assert!(!ui.contains_key("focused_key"));
    }

    #[test]
    fn semantic_settings_intent_updates_the_view_without_a_vm_slider() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.ui"), SIZE).unwrap();
        vm.state.ui.route = "settings".to_owned();
        let mut input = InputSnapshot::idle(1, 16);
        input.intents.push(UiIntent::SetSetting {
            name: "text_scale".to_owned(),
            value: 1.25,
        });
        input.intents.push(UiIntent::SetSetting {
            name: "text_opacity".to_owned(),
            value: 0.76,
        });
        input.intents.push(UiIntent::ToggleSetting {
            name: "high_contrast".to_owned(),
        });
        input.intents.push(UiIntent::ToggleSetting {
            name: "skip_unread".to_owned(),
        });
        input.intents.push(UiIntent::ToggleSetting {
            name: "stage_effects".to_owned(),
        });
        let output = vm.step(&input).unwrap();
        assert_eq!(output.view.route, UiRoute::Settings);
        assert_eq!(output.view.settings.text_scale, 1.25);
        assert_eq!(output.view.settings.text_opacity, 0.76);
        assert!(output.view.settings.high_contrast);
        assert!(output.view.settings.skip_unread);
        assert!(!output.view.settings.stage_effects);
        assert!(output.scene.commands.is_empty());
    }

    #[test]
    fn automatic_checkpoint_is_not_a_player_facing_record() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.autosave"), SIZE).unwrap();
        vm.state.ui.route = "load".to_owned();
        let load = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert!(
            load.view
                .actions
                .iter()
                .all(|action| action.id != "load.slot.0")
        );

        let mut automatic = InputSnapshot::idle(2, 16);
        automatic.intents.push(UiIntent::Activate {
            id: "load.slot.0".to_owned(),
        });
        assert!(matches!(
            vm.step(&automatic),
            Err(VmError::InvalidUiIntent(message)) if message.contains("load.slot.0")
        ));

        let mut manual = Vm::new(CompiledProgram::empty("jp.example.autosave"), SIZE).unwrap();
        let mut forbidden = InputSnapshot::idle(1, 16);
        forbidden.intents.push(UiIntent::Activate {
            id: "save.slot.0".to_owned(),
        });
        assert!(matches!(
            manual.step(&forbidden),
            Err(VmError::InvalidUiIntent(message)) if message.contains("save.slot.0")
        ));
    }

    #[test]
    fn snapshots_retain_reading_preferences_and_both_flag_scopes() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.snapshot"), SIZE).unwrap();
        vm.state.flags.insert("tide_seen".to_owned(), true);
        vm.state
            .persistent_flags
            .insert("chapter_unlocked".to_owned(), true);
        vm.state.read_texts.insert("line-001".to_owned());
        vm.state.settings.text_opacity = 0.84;
        vm.state.settings.stage_effects = false;

        let snapshot = vm.snapshot();
        assert!(snapshot.flags["tide_seen"]);
        assert!(snapshot.persistent_flags["chapter_unlocked"]);
        assert!(snapshot.read_texts.contains("line-001"));
        assert_eq!(snapshot.settings.text_opacity, 0.84);
        assert!(!snapshot.settings.stage_effects);
    }

    #[test]
    fn skip_uses_the_unread_preference_from_settings() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.skip"), SIZE).unwrap();
        let mut enable = InputSnapshot::idle(1, 16);
        enable.intents.push(UiIntent::ToggleSetting {
            name: "skip_unread".to_owned(),
        });
        vm.step(&enable).unwrap();

        let mut skip = InputSnapshot::idle(2, 16);
        skip.intents.push(UiIntent::Activate {
            id: "menu.skip".to_owned(),
        });
        let output = vm.step(&skip).unwrap();
        assert_eq!(output.view.skip_mode, SkipMode::All);
    }

    #[test]
    fn route_stack_and_cinematic_scene_transition_are_semantic() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.ui"), SIZE).unwrap();
        let output = vm
            .step(&InputSnapshot::pressed(1, 16, InputAction::Menu))
            .unwrap();
        assert_eq!(output.view.route, UiRoute::Pause);
        assert!(matches!(
            output.scene.transition,
            Some(TransitionFrame {
                kind: TransitionKind::Fade,
                ..
            })
        ));
        let mut dismiss = InputSnapshot::idle(2, 16);
        dismiss.intents.push(UiIntent::Dismiss);
        let closed = vm.step(&dismiss).unwrap();
        assert_eq!(closed.view.route, UiRoute::Dialogue);
    }

    #[test]
    fn rmenu_exposes_full_save_range_and_confirms_reset_before_runtime_exit() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.rmenu"), SIZE).unwrap();
        let pause = vm
            .step(&InputSnapshot::pressed(1, 16, InputAction::Menu))
            .unwrap();
        assert_eq!(pause.view.route, UiRoute::Pause);
        for id in [
            "menu.save",
            "menu.load",
            "menu.backlog",
            "menu.gallery",
            "menu.settings",
            "menu.skip",
            "menu.reset",
            "menu.quit",
        ] {
            assert!(
                pause.view.actions.iter().any(|action| action.id == id),
                "missing {id}"
            );
        }

        let mut save = InputSnapshot::idle(2, 16);
        save.intents.push(UiIntent::Activate {
            id: "menu.save".to_owned(),
        });
        let save = vm.step(&save).unwrap();
        assert_eq!(save.view.route, UiRoute::Save);
        assert!(
            save.view
                .actions
                .iter()
                .any(|action| action.id == "save.slot.10")
        );

        let mut save_ten = InputSnapshot::idle(3, 16);
        save_ten.intents.push(UiIntent::Activate {
            id: "save.slot.10".to_owned(),
        });
        let after_save = vm.step(&save_ten).unwrap();
        assert_eq!(after_save.view.route, UiRoute::Dialogue);
        assert!(
            after_save
                .runtime
                .contains(&RuntimeCommand::Save { slot: 10 })
        );

        let _pause = vm
            .step(&InputSnapshot::pressed(4, 16, InputAction::Menu))
            .unwrap();
        let mut reset = InputSnapshot::idle(5, 16);
        reset.intents.push(UiIntent::Activate {
            id: "menu.reset".to_owned(),
        });
        let confirm = vm.step(&reset).unwrap();
        assert_eq!(confirm.view.route, UiRoute::Confirm);
        assert_eq!(
            confirm.view.confirmation,
            Some(ConfirmationView {
                action: "reset".to_owned(),
                resume_id: None,
            })
        );

        let mut cancel = InputSnapshot::idle(6, 16);
        cancel.intents.push(UiIntent::Activate {
            id: "confirm.cancel".to_owned(),
        });
        let after_cancel = vm.step(&cancel).unwrap();
        assert_eq!(after_cancel.view.route, UiRoute::Pause);
        assert_eq!(after_cancel.view.confirmation, None);

        let mut reset_again = InputSnapshot::idle(7, 16);
        reset_again.intents.push(UiIntent::Activate {
            id: "menu.reset".to_owned(),
        });
        let _confirm = vm.step(&reset_again).unwrap();
        let mut accept = InputSnapshot::idle(8, 16);
        accept.intents.push(UiIntent::Activate {
            id: "confirm.accept".to_owned(),
        });
        let after_accept = vm.step(&accept).unwrap();
        assert_eq!(after_accept.view.route, UiRoute::Dialogue);
        assert!(
            after_accept
                .runtime
                .contains(&RuntimeCommand::ReturnToTitle)
        );
    }

    #[test]
    fn rmenu_auto_and_skip_toggle_return_to_reading_with_their_state() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.rmenu.modes"), SIZE).unwrap();
        let _pause = vm
            .step(&InputSnapshot::pressed(1, 16, InputAction::Menu))
            .unwrap();
        let mut auto = InputSnapshot::idle(2, 16);
        auto.intents.push(UiIntent::Activate {
            id: "menu.auto".to_owned(),
        });
        let after_auto = vm.step(&auto).unwrap();
        assert_eq!(after_auto.view.route, UiRoute::Dialogue);
        assert!(
            after_auto
                .view
                .actions
                .iter()
                .any(|action| action.id == "menu.auto" && action.active)
        );

        let _pause = vm
            .step(&InputSnapshot::pressed(3, 16, InputAction::Menu))
            .unwrap();
        let mut skip = InputSnapshot::idle(4, 16);
        skip.intents.push(UiIntent::Activate {
            id: "menu.skip".to_owned(),
        });
        let after_skip = vm.step(&skip).unwrap();
        assert_eq!(after_skip.view.route, UiRoute::Dialogue);
        assert!(
            after_skip
                .view
                .actions
                .iter()
                .any(|action| action.id == "menu.skip" && action.active)
        );
    }

    #[test]
    fn semantic_scroll_is_replayed_and_persisted() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.ui"), SIZE).unwrap();
        vm.state.ui.route = "backlog".to_owned();
        vm.state.backlog = (0..12)
            .map(|index| BacklogEntryState {
                id: format!("line-{index}"),
                speaker: None,
                text: format!("line {index}"),
                locale: "ja-JP".to_owned(),
                timestamp_ms: index,
            })
            .collect();
        let mut input = InputSnapshot::idle(1, 16);
        input.intents.push(UiIntent::Scroll {
            region: "backlog".to_owned(),
            delta_y: 88.0,
        });
        let output = vm.step(&input).unwrap();
        assert!(output.view.scroll_offsets["backlog"] >= 88.0 / 48.0);
        assert_eq!(vm.snapshot().ui.scroll_offsets, output.view.scroll_offsets);
    }

    #[test]
    fn subtitle_pages_are_two_lines_wide_and_advance_before_the_story_vm() {
        let sources = [
            ("ja", "海風が止んだ。次の駅まで、まだ時間がある。"),
            (
                "en",
                "The tide carries every quiet sentence toward the next station. ",
            ),
            ("zh", "海風仍在窗外慢慢退去，下一站還很遠。"),
        ];
        for (language, sample) in sources {
            let source = sample.chars().cycle().take(297).collect::<String>();
            let script = format!(
                "aria;\nentry start;\nscene start {{ screen dialogue; narrate \"{source}\"; await advance; narrate \"次の場面\"; await advance; end; }}\n"
            );
            for width in [1_440, 720, 390] {
                let mut runtime = vm(&script);
                let first = runtime
                    .step(&InputSnapshot::idle(1, 16).with_viewport(UiViewport {
                        width,
                        height: 844,
                        scale_factor: 1.0,
                        ..UiViewport::default()
                    }))
                    .unwrap();
                let dialogue = first.view.dialogue.expect("subtitle page");
                assert!(dialogue.page_count > 1, "{language}, width {width}");
                assert_eq!(dialogue.page_number, 1);
                assert!(dialogue.full_page_text.lines().count() <= 2);
                assert!(dialogue.full_page_text.lines().all(|line| {
                    line.chars()
                        .map(|character| if character.is_ascii() { 1 } else { 2 })
                        .sum::<usize>()
                        <= usize::from(dialogue.columns)
                }));
                assert!(dialogue.text.is_empty());

                let mut reveal = InputSnapshot::idle(2, 16);
                reveal.intents.push(UiIntent::Activate {
                    id: "dialogue.advance".to_owned(),
                });
                let revealed = runtime.step(&reveal).unwrap();
                let revealed_dialogue = revealed.view.dialogue.expect("revealed page");
                assert_eq!(revealed_dialogue.text, revealed_dialogue.full_page_text);
                assert_eq!(runtime.backlog().len(), 1);
                // History is intentionally absent from a normal typewriter update.
                assert!(revealed.view.backlog.is_empty());

                let mut next = InputSnapshot::idle(3, 16);
                next.intents.push(UiIntent::Activate {
                    id: "dialogue.advance".to_owned(),
                });
                let next_page = runtime
                    .step(&next)
                    .unwrap()
                    .view
                    .dialogue
                    .expect("next page");
                assert_eq!(next_page.page_number, 2);
                assert!(next_page.text.is_empty());
                assert!(next_page.full_page_text.lines().count() <= 2);
            }
        }
    }

    #[test]
    fn backlog_resume_replays_choice_trace_and_survives_a_save_confirmation() {
        let mut runtime = vm("aria;\n\
             entry start;\n\
             scene start { screen dialogue; choice { \"海へ\" => sea; \"空へ\" => sky; } }\n\
             scene sea { persistent flag \"sea_seen\" = true; unlock chapter \"sea\" progress 100; unlock cg \"sea-cg\"; narrate \"最初の潮目。\"; await advance; narrate \"この先は新しい分岐です。\"; await advance; end; }\n\
             scene sky { narrate \"空の分岐。\"; await advance; end; }\n");
        let _choice = runtime.step(&InputSnapshot::idle(1, 16)).unwrap();
        let mut select_sea = InputSnapshot::idle(2, 16);
        select_sea.intents.push(UiIntent::Activate {
            id: "choice:0".to_owned(),
        });
        let _first_page = runtime.step(&select_sea).unwrap();

        let mut reveal_first = InputSnapshot::idle(3, 16);
        reveal_first.intents.push(UiIntent::Activate {
            id: "dialogue.advance".to_owned(),
        });
        runtime.step(&reveal_first).unwrap();
        let first_id = runtime.backlog()[0].id.clone();

        let mut move_to_second = InputSnapshot::idle(4, 16);
        move_to_second.intents.push(UiIntent::Activate {
            id: "dialogue.advance".to_owned(),
        });
        runtime.step(&move_to_second).unwrap();
        let mut reveal_second = InputSnapshot::idle(5, 16);
        reveal_second.intents.push(UiIntent::Activate {
            id: "dialogue.advance".to_owned(),
        });
        runtime.step(&reveal_second).unwrap();
        assert_eq!(runtime.backlog().len(), 2);
        assert!(runtime.snapshot().chapters.contains_key("sea"));
        assert!(runtime.snapshot().unlocked_cgs.contains("sea-cg"));

        let mut open_backlog = InputSnapshot::idle(6, 16);
        open_backlog.intents.push(UiIntent::Activate {
            id: "chrome.backlog".to_owned(),
        });
        let backlog = runtime.step(&open_backlog).unwrap();
        assert_eq!(backlog.view.route, UiRoute::Backlog);
        assert_eq!(backlog.view.backlog[0].resume_id, first_id);

        let mut choose_entry = InputSnapshot::idle(7, 16);
        choose_entry.intents.push(UiIntent::Activate {
            id: format!("backlog:{first_id}"),
        });
        let confirmation = runtime.step(&choose_entry).unwrap();
        assert_eq!(confirmation.view.route, UiRoute::Confirm);
        assert_eq!(
            confirmation.view.confirmation,
            Some(ConfirmationView {
                action: "resume_backlog".to_owned(),
                resume_id: Some(first_id.clone()),
            })
        );

        let snapshot = runtime.snapshot();
        let mut resumed = Vm::new(runtime.program.clone(), SIZE).unwrap();
        resumed.restore(snapshot).unwrap();
        let mut accept = InputSnapshot::idle(1, 16);
        accept.intents.push(UiIntent::Activate {
            id: "confirm.accept".to_owned(),
        });
        let output = resumed.step(&accept).unwrap();
        let dialogue = output.view.dialogue.expect("resumed subtitle");
        assert_eq!(output.view.route, UiRoute::Dialogue);
        assert_eq!(dialogue.page_id, first_id);
        assert_eq!(dialogue.text, dialogue.full_page_text);
        assert_eq!(resumed.backlog().len(), 1);
        assert_eq!(resumed.snapshot().narrative_trace.len(), 1);
        assert!(resumed.snapshot().chapters.contains_key("sea"));
        assert!(resumed.snapshot().unlocked_cgs.contains("sea-cg"));
    }

    #[test]
    fn gallery_viewer_is_a_saved_topmost_layer_with_previous_and_next() {
        let mut runtime = Vm::new(CompiledProgram::empty("jp.example.gallery"), SIZE).unwrap();
        runtime.state.ui.route = "gallery".to_owned();
        runtime.state.unlocked_cgs = BTreeSet::from(["cg-a".to_owned(), "cg-b".to_owned()]);
        let mut open = InputSnapshot::idle(1, 16);
        open.intents.push(UiIntent::Activate {
            id: "gallery:cg-a".to_owned(),
        });
        let opened = runtime.step(&open).unwrap();
        assert_eq!(opened.view.gallery_viewer.as_deref(), Some("cg-a"));
        assert!(opened.view.gallery.iter().any(|item| item.selected));

        let mut next = InputSnapshot::idle(2, 16);
        next.intents.push(UiIntent::Activate {
            id: "gallery.next".to_owned(),
        });
        let next = runtime.step(&next).unwrap();
        assert_eq!(next.view.gallery_viewer.as_deref(), Some("cg-b"));

        let snapshot = runtime.snapshot();
        let mut restored = Vm::new(CompiledProgram::empty("jp.example.gallery"), SIZE).unwrap();
        restored.restore(snapshot).unwrap();
        let mut dismiss = InputSnapshot::idle(1, 16);
        dismiss.intents.push(UiIntent::Dismiss);
        let grid = restored.step(&dismiss).unwrap();
        assert_eq!(grid.view.route, UiRoute::Gallery);
        assert_eq!(grid.view.gallery_viewer, None);
    }

    #[test]
    fn old_snapshots_are_rejected_after_the_visual_ui_break() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.ui"), SIZE).unwrap();
        let mut old = vm.snapshot();
        old.schema_version = 6;
        assert!(matches!(
            vm.restore(old),
            Err(VmError::UnsupportedSnapshot(6))
        ));
    }

    #[test]
    fn previous_confirmation_shape_reaches_the_explicit_schema_diagnostic() {
        let mut vm = Vm::new(CompiledProgram::empty("jp.example.ui"), SIZE).unwrap();
        let mut previous = serde_json::to_value(vm.snapshot()).unwrap();
        previous["schema_version"] = serde_json::json!(7);
        previous["ui"]["confirmation"] = serde_json::json!("reset");
        let old: VmSnapshot = serde_json::from_value(previous).unwrap();

        assert!(matches!(
            vm.restore(old),
            Err(VmError::UnsupportedSnapshot(7))
        ));
    }
}
