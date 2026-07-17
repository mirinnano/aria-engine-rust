use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bytecode::{ByteOp, CompiledProgram, Constant, EncodedInstruction, Operand};
use crate::input::{InputAction, InputSnapshot};
use crate::protocol::{
    AudioBus, AudioCommand, BlendMode, Color, DrawCommand, LogicalSize, Rect, RenderFrame,
    RuntimeCommand, StepOutput, TransitionFrame, TransitionKind, UiActivation, UiNode, UiRole,
    UiTree,
};
use crate::text::{grapheme_count, grapheme_prefix};

const VM_SNAPSHOT_SCHEMA: u32 = 3;
const MAX_INSTRUCTIONS_PER_TICK: usize = 100_000;
/// A malformed bytecode program must not turn an unbounded recursive Call
/// cycle into host-memory exhaustion. Aria 3.1 rejects recursive calls at
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
    pub halted: bool,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextState {
    pub speaker: Option<String>,
    pub full_text: String,
    pub visible_graphemes: usize,
    pub reveal_elapsed_ms: u32,
    pub speed_ms: u32,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            speaker: None,
            full_text: String::new(),
            visible_graphemes: 0,
            reveal_elapsed_ms: 0,
            speed_ms: 24,
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
                halted: false,
            },
            pending_audio: Vec::new(),
            pending_runtime: Vec::new(),
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

    #[must_use]
    pub fn logical_size(&self) -> LogicalSize {
        self.logical_size
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
        self.state.frame_number = self.state.frame_number.saturating_add(1);
        self.state.logical_time_ms = self
            .state
            .logical_time_ms
            .saturating_add(u64::from(input.delta_ms));
        self.update_transition(input.delta_ms);
        self.update_typewriter(input.delta_ms);
        self.handle_waiting_input(input)?;

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

        if input.is_pressed(InputAction::Menu) || input.is_pressed(InputAction::Cancel) {
            self.pending_runtime.push(RuntimeCommand::OpenMenu);
        }

        let render = self.render_frame();
        let ui = self.ui_tree();
        Ok(StepOutput {
            render,
            audio: std::mem::take(&mut self.pending_audio),
            ui,
            runtime: std::mem::take(&mut self.pending_runtime),
            halted: self.state.halted,
        })
    }

    fn update_transition(&mut self, delta_ms: u32) {
        if let Some(transition) = &mut self.state.transition {
            transition.elapsed_ms = transition.elapsed_ms.saturating_add(delta_ms);
            if transition.elapsed_ms >= transition.duration_ms {
                self.state.transition = None;
            }
        }
    }

    fn update_typewriter(&mut self, delta_ms: u32) {
        let total = grapheme_count(&self.state.text.full_text);
        if self.state.text.visible_graphemes >= total {
            return;
        }
        if self.state.text.speed_ms == 0 {
            self.state.text.visible_graphemes = total;
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
    }

    fn handle_waiting_input(&mut self, input: &InputSnapshot) -> Result<(), VmError> {
        match self.state.execution.clone() {
            ExecutionState::Running => {}
            ExecutionState::WaitingForDelay { remaining_ms } => {
                let remaining = remaining_ms.saturating_sub(input.delta_ms);
                if remaining == 0 || input.is_held(InputAction::Skip) {
                    self.state.execution = ExecutionState::Running;
                } else {
                    self.state.execution = ExecutionState::WaitingForDelay {
                        remaining_ms: remaining,
                    };
                }
            }
            ExecutionState::WaitingForAdvance { clear_page } => {
                let activated = input.is_pressed(InputAction::Advance)
                    || input.is_pressed(InputAction::Confirm)
                    || input.pointer.is_some_and(|pointer| pointer.primary_pressed)
                    || input.is_held(InputAction::Skip);
                if activated {
                    let total = grapheme_count(&self.state.text.full_text);
                    if self.state.text.visible_graphemes < total {
                        self.state.text.visible_graphemes = total;
                    } else {
                        if clear_page {
                            self.clear_text();
                        }
                        self.state.execution = ExecutionState::Running;
                    }
                }
            }
            ExecutionState::WaitingForChoice => self.handle_choice_input(input)?,
        }
        Ok(())
    }

    fn handle_choice_input(&mut self, input: &InputSnapshot) -> Result<(), VmError> {
        let Some(choice) = &mut self.state.choice else {
            return Err(self.error_at_pc(VmErrorKind::MissingChoiceState));
        };
        if choice.options.is_empty() {
            return Err(self.error_at_pc(VmErrorKind::MissingChoiceState));
        }
        if input.is_pressed(InputAction::NavigateUp) || input.is_pressed(InputAction::NavigateLeft)
        {
            choice.focused = if choice.focused == 0 {
                choice.options.len() - 1
            } else {
                choice.focused - 1
            };
        }
        if input.is_pressed(InputAction::NavigateDown)
            || input.is_pressed(InputAction::NavigateRight)
        {
            choice.focused = (choice.focused + 1) % choice.options.len();
        }
        if let Some(pointer) = input.pointer {
            for (index, bounds) in choice_bounds(self.logical_size, choice.options.len())
                .into_iter()
                .enumerate()
            {
                if bounds.contains(pointer.x, pointer.y) {
                    choice.focused = index;
                    if pointer.primary_pressed {
                        break;
                    }
                }
            }
        }
        let pointer_confirmed = input.pointer.is_some_and(|pointer| {
            pointer.primary_pressed
                && choice_bounds(self.logical_size, choice.options.len())
                    .get(choice.focused)
                    .is_some_and(|bounds| bounds.contains(pointer.x, pointer.y))
        });
        if input.is_pressed(InputAction::Confirm)
            || input.is_pressed(InputAction::Advance)
            || pointer_confirmed
        {
            let selected = choice.focused;
            let target = choice.options[selected].target;
            self.state
                .int_registers
                .insert("choice".to_owned(), selected as i64 + 1);
            self.state.pc = target;
            self.state.choice = None;
            self.state.execution = ExecutionState::Running;
        }
        Ok(())
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
                self.state.text = TextState {
                    speaker,
                    full_text: text,
                    visible_graphemes: 0,
                    reveal_elapsed_ms: 0,
                    speed_ms: self.state.text.speed_ms,
                };
            }
            ByteOp::WaitAdvance => {
                let clear_page = self.boolean(self.operand(instruction, 0)?)?;
                self.state.execution = ExecutionState::WaitingForAdvance { clear_page };
            }
            ByteOp::TextClear => self.clear_text(),
            ByteOp::Delay => {
                let duration = self.integer(self.operand(instruction, 0)?)?.max(0) as u32;
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
            ByteOp::Host => {
                let name = self.string(self.operand(instruction, 0)?)?;
                let arguments = self.string(self.operand(instruction, 1)?)?;
                self.pending_runtime
                    .push(RuntimeCommand::Unsupported { name, arguments });
            }
        }
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

    fn render_frame(&self) -> RenderFrame {
        let mut commands = Vec::new();
        if let Some(background) = &self.state.background {
            let bounds = Rect {
                x: 0.0,
                y: 0.0,
                width: self.logical_size.width as f32,
                height: self.logical_size.height as f32,
            };
            if let Some(color) = parse_color(&background.asset) {
                commands.push(DrawCommand::Rectangle {
                    id: "background".to_owned(),
                    bounds,
                    color,
                    corner_radius: 0.0,
                    z: -10_000,
                });
            } else {
                commands.push(DrawCommand::Sprite {
                    id: "background".to_owned(),
                    asset: background.asset.clone(),
                    destination: bounds,
                    opacity: 255,
                    z: -10_000,
                    visible: true,
                    blend: BlendMode::Alpha,
                    mask: None,
                });
            }
        }
        for sprite in self.state.sprites.values() {
            match sprite.kind {
                SpriteKind::Image => commands.push(DrawCommand::Sprite {
                    id: sprite.id.clone(),
                    asset: sprite.content.clone(),
                    destination: sprite.bounds,
                    opacity: sprite.opacity,
                    z: sprite.z,
                    visible: sprite.visible,
                    blend: BlendMode::Alpha,
                    mask: None,
                }),
                SpriteKind::Rectangle if sprite.visible => {
                    commands.push(DrawCommand::Rectangle {
                        id: sprite.id.clone(),
                        bounds: sprite.bounds,
                        color: sprite.color,
                        corner_radius: 0.0,
                        z: sprite.z,
                    });
                }
                SpriteKind::Text if sprite.visible => commands.push(DrawCommand::Text {
                    id: sprite.id.clone(),
                    text: sprite.content.clone(),
                    speaker: None,
                    bounds: sprite.bounds,
                    color: sprite.color,
                    font_size: sprite.font_size,
                    z: sprite.z,
                }),
                _ => {}
            }
        }
        if !self.state.text.full_text.is_empty() {
            let textbox = textbox_bounds(self.logical_size);
            commands.push(DrawCommand::Rectangle {
                id: "vn.textbox.background".to_owned(),
                bounds: textbox,
                color: Color {
                    red: 5,
                    green: 8,
                    blue: 12,
                    alpha: 220,
                },
                corner_radius: 8.0,
                z: 9_000,
            });
            let inset = 28.0;
            commands.push(DrawCommand::Text {
                id: "vn.textbox.text".to_owned(),
                text: grapheme_prefix(
                    &self.state.text.full_text,
                    self.state.text.visible_graphemes,
                ),
                speaker: self.state.text.speaker.clone(),
                bounds: Rect {
                    x: textbox.x + inset,
                    y: textbox.y + inset,
                    width: textbox.width - inset * 2.0,
                    height: textbox.height - inset * 2.0,
                },
                color: Color::WHITE,
                font_size: 28.0,
                z: 9_010,
            });
        }
        if let Some(choice) = &self.state.choice {
            for (index, (option, bounds)) in choice
                .options
                .iter()
                .zip(choice_bounds(self.logical_size, choice.options.len()))
                .enumerate()
            {
                commands.push(DrawCommand::Rectangle {
                    id: format!("vn.choice.{index}.background"),
                    bounds,
                    color: if index == choice.focused {
                        Color {
                            red: 62,
                            green: 88,
                            blue: 118,
                            alpha: 245,
                        }
                    } else {
                        Color {
                            red: 20,
                            green: 28,
                            blue: 38,
                            alpha: 235,
                        }
                    },
                    corner_radius: 6.0,
                    z: 9_100,
                });
                commands.push(DrawCommand::Text {
                    id: format!("vn.choice.{index}.label"),
                    text: option.text.clone(),
                    speaker: None,
                    bounds,
                    color: Color::WHITE,
                    font_size: 24.0,
                    z: 9_110,
                });
            }
        }
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
        let mut frame = RenderFrame {
            frame_number: self.state.frame_number,
            logical_size: self.logical_size,
            clear_color: Color::BLACK,
            commands,
            transition,
        };
        frame.sort_commands();
        frame
    }

    fn ui_tree(&self) -> UiTree {
        let full = Rect {
            x: 0.0,
            y: 0.0,
            width: self.logical_size.width as f32,
            height: self.logical_size.height as f32,
        };
        let mut nodes = BTreeMap::new();
        let mut root_children = Vec::new();
        if !self.state.text.full_text.is_empty() {
            root_children.push(2);
            nodes.insert(
                2,
                UiNode {
                    id: 2,
                    role: UiRole::Dialog,
                    label: self.state.text.full_text.clone(),
                    bounds: textbox_bounds(self.logical_size),
                    focusable: false,
                    focused: false,
                    activation: Some(UiActivation::Input(InputAction::Advance)),
                    children: Vec::new(),
                },
            );
        }
        if let Some(choice) = &self.state.choice {
            let mut choice_children = Vec::new();
            for (index, (option, bounds)) in choice
                .options
                .iter()
                .zip(choice_bounds(self.logical_size, choice.options.len()))
                .enumerate()
            {
                let id = 100 + index as u64;
                choice_children.push(id);
                nodes.insert(
                    id,
                    UiNode {
                        id,
                        role: UiRole::Button,
                        label: option.text.clone(),
                        bounds,
                        focusable: true,
                        focused: index == choice.focused,
                        activation: Some(UiActivation::SelectChoice(index as u32)),
                        children: Vec::new(),
                    },
                );
            }
            root_children.push(3);
            nodes.insert(
                3,
                UiNode {
                    id: 3,
                    role: UiRole::Group,
                    label: "選択肢".to_owned(),
                    bounds: full,
                    focusable: false,
                    focused: false,
                    activation: None,
                    children: choice_children,
                },
            );
        }
        nodes.insert(
            1,
            UiNode {
                id: 1,
                role: UiRole::Window,
                label: self.program.game_id.clone(),
                bounds: full,
                focusable: false,
                focused: false,
                activation: None,
                children: root_children,
            },
        );
        UiTree {
            root: 1,
            nodes,
            scale_factor: 1.0,
            safe_area: full,
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

fn textbox_bounds(size: LogicalSize) -> Rect {
    let margin = (size.width as f32 * 0.035).max(16.0);
    let height = (size.height as f32 * 0.28).max(140.0);
    Rect {
        x: margin,
        y: size.height as f32 - height - margin,
        width: size.width as f32 - margin * 2.0,
        height,
    }
}

fn choice_bounds(size: LogicalSize, count: usize) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let width = (size.width as f32 * 0.58).clamp(280.0, 760.0);
    let height = (size.height as f32 * 0.075).max(48.0);
    let gap = 12.0;
    let total = count as f32 * height + count.saturating_sub(1) as f32 * gap;
    let x = (size.width as f32 - width) / 2.0;
    let start_y = ((size.height as f32 - total) / 2.0).max(16.0);
    (0..count)
        .map(|index| Rect {
            x,
            y: start_y + index as f32 * (height + gap),
            width,
            height,
        })
        .collect()
}

#[derive(Debug, Error)]
pub enum VmError {
    #[error("invalid compiled program: {0}")]
    InvalidProgram(String),
    #[error("logical resolution must be non-zero")]
    InvalidLogicalSize,
    #[error("input sequence must increase (previous {previous}, received {received})")]
    NonMonotonicInput { previous: u64, received: u64 },
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
    use crate::bytecode::{
        ARIAC_FORMAT_VERSION, ByteOp, CompiledProgram, EncodedInstruction, LanguageVersion,
        Operand, SourceLocation,
    };
    use crate::compiler::{CompileInput, SourceUnit, compile};

    use super::*;

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
        Vm::new(
            compiled.program.unwrap(),
            LogicalSize {
                width: 1280,
                height: 720,
            },
        )
        .unwrap()
    }

    #[test]
    fn dialogue_typewriter_choice_and_audio_are_deterministic() {
        let mut vm = vm("# aria-version: 3.0\n\
             bg \"sea.webp\", 300\n\
             lsp 1, \"mio.webp\", 700, 80\n\
             ミオ「海へ行こう。」\n\
             choice \"行く\", *go, \"戻る\", *end\n\
             *go\n\
             play_bgm \"sea.ogg\"\n\
             *end\n\
             end\n");
        let first = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
        assert!(first.render.commands.iter().any(|command| {
            matches!(command, DrawCommand::Sprite { asset, .. } if asset == "mio.webp")
        }));
        assert!(!first.halted);

        vm.step(&InputSnapshot::idle(2, 16)).unwrap();
        let choice = vm.step(&InputSnapshot::idle(3, 16)).unwrap();
        assert!(
            choice
                .ui
                .nodes
                .values()
                .any(|node| node.role == UiRole::Button)
        );
        let selected = vm
            .step(&InputSnapshot::pressed(4, 16, InputAction::Confirm))
            .unwrap();
        assert!(selected.audio.iter().any(|command| {
            matches!(
                command,
                AudioCommand::Play {
                    bus: AudioBus::Bgm,
                    ..
                }
            )
        }));
        assert!(selected.halted);
    }

    #[test]
    fn snapshot_restore_replays_identically() {
        let source = "# aria-version: 3.0\nミオ「保存する？」\ntext \"復帰した。\"\n@\nend\n";
        let mut first = vm(source);
        first.step(&InputSnapshot::idle(1, 50)).unwrap();
        let snapshot = first.snapshot();
        let expected = first
            .step(&InputSnapshot::pressed(2, 50, InputAction::Advance))
            .unwrap();

        let mut restored = vm(source);
        restored.restore(snapshot).unwrap();
        let actual = restored
            .step(&InputSnapshot::pressed(2, 50, InputAction::Advance))
            .unwrap();
        assert_eq!(actual.render, expected.render);
        assert_eq!(actual.ui, expected.ui);
        assert_eq!(actual.runtime, expected.runtime);
        assert_eq!(actual.halted, expected.halted);
        assert_eq!(actual.audio.len(), 3, "restore reapplies bus volumes");
        assert_eq!(restored.snapshot(), first.snapshot());
    }

    #[test]
    fn restore_reemits_active_audio_tracks_without_a_second_fade() {
        let mut original = vm("# aria-version: 3.0\nplay_bgm \"sea.ogg\"\n@\nend\n");
        original.step(&InputSnapshot::idle(1, 16)).unwrap();
        let snapshot = original.snapshot();

        let mut restored = vm("# aria-version: 3.0\nplay_bgm \"sea.ogg\"\n@\nend\n");
        restored.restore(snapshot).unwrap();
        let output = restored.step(&InputSnapshot::idle(2, 16)).unwrap();
        assert!(output.audio.iter().any(|command| {
            matches!(
                command,
                AudioCommand::Play {
                    bus: AudioBus::Bgm,
                    asset,
                    looping: true,
                    fade_in_ms: 0,
                    ..
                } if asset == "sea.ogg"
            )
        }));
    }

    #[test]
    fn gamepad_navigation_actions_select_the_same_semantic_choice() {
        let mut vm = vm(
            "# aria-version: 3.0\nchoice \"A\", *a, \"B\", *b\n*a\nlet %route, 1\ngoto *end\n*b\nlet %route, 2\n*end\n@\nend\n",
        );
        vm.step(&InputSnapshot::idle(1, 16)).unwrap();
        vm.step(&InputSnapshot::pressed(2, 16, InputAction::NavigateDown))
            .unwrap();
        vm.step(&InputSnapshot::pressed(3, 16, InputAction::Confirm))
            .unwrap();
        assert_eq!(vm.snapshot().int_registers.get("route"), Some(&2));
    }

    #[test]
    fn malformed_bytecode_cannot_return_without_a_caller_or_recurse_forever() {
        let source_map = vec![SourceLocation {
            source: "generated.aria".to_owned(),
            line: 1,
            column: 1,
        }];
        let return_program = CompiledProgram {
            format_version: ARIAC_FORMAT_VERSION,
            language_version: LanguageVersion::V3_1,
            game_id: "jp.example.vm-guard".to_owned(),
            constants: Vec::new(),
            instructions: vec![EncodedInstruction::new(ByteOp::Return, Vec::new())],
            source_map: source_map.clone(),
        };
        let mut vm = Vm::new(
            return_program,
            LogicalSize {
                width: 640,
                height: 360,
            },
        )
        .unwrap();
        assert!(matches!(
            vm.step(&InputSnapshot::idle(1, 0)),
            Err(VmError::Runtime {
                kind: VmErrorKind::ReturnWithoutCaller,
                ..
            })
        ));

        let recursive_program = CompiledProgram {
            format_version: ARIAC_FORMAT_VERSION,
            language_version: LanguageVersion::V3_1,
            game_id: "jp.example.vm-guard".to_owned(),
            constants: Vec::new(),
            instructions: vec![EncodedInstruction::new(
                ByteOp::Call,
                vec![Operand::Address(0)],
            )],
            source_map,
        };
        let mut vm = Vm::new(
            recursive_program,
            LogicalSize {
                width: 640,
                height: 360,
            },
        )
        .unwrap();
        assert!(matches!(
            vm.step(&InputSnapshot::idle(1, 0)),
            Err(VmError::Runtime {
                kind: VmErrorKind::CallDepthExceeded,
                ..
            })
        ));
    }
}
