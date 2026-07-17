use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::input::InputAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        red: 0,
        green: 0,
        blue: 0,
        alpha: 255,
    };

    pub const WHITE: Self = Self {
        red: 255,
        green: 255,
        blue: 255,
        alpha: 255,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    #[must_use]
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    Alpha,
    Add,
    Multiply,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawCommand {
    Sprite {
        id: String,
        asset: String,
        destination: Rect,
        opacity: u8,
        z: i32,
        visible: bool,
        blend: BlendMode,
        mask: Option<String>,
    },
    Rectangle {
        id: String,
        bounds: Rect,
        color: Color,
        corner_radius: f32,
        z: i32,
    },
    Text {
        id: String,
        text: String,
        speaker: Option<String>,
        bounds: Rect,
        color: Color,
        font_size: f32,
        z: i32,
    },
}

impl DrawCommand {
    #[must_use]
    pub const fn z(&self) -> i32 {
        match self {
            Self::Sprite { z, .. } | Self::Rectangle { z, .. } | Self::Text { z, .. } => *z,
        }
    }

    #[must_use]
    pub fn stable_id(&self) -> &str {
        match self {
            Self::Sprite { id, .. } | Self::Rectangle { id, .. } | Self::Text { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Fade,
    CrossFade,
    WipeLeft,
    WipeRight,
    Mask(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionFrame {
    pub kind: TransitionKind,
    pub progress: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderFrame {
    pub frame_number: u64,
    pub logical_size: LogicalSize,
    pub clear_color: Color,
    pub commands: Vec<DrawCommand>,
    pub transition: Option<TransitionFrame>,
}

impl RenderFrame {
    pub fn sort_commands(&mut self) {
        self.commands.sort_by(|left, right| {
            left.z()
                .cmp(&right.z())
                .then_with(|| left.stable_id().cmp(right.stable_id()))
        });
    }

    #[must_use]
    pub fn digest(&self) -> String {
        stable_digest(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioBus {
    Bgm,
    SoundEffect,
    Voice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioCommand {
    Play {
        bus: AudioBus,
        id: String,
        asset: String,
        looping: bool,
        volume: f32,
        fade_in_ms: u32,
    },
    Stop {
        bus: AudioBus,
        id: Option<String>,
        fade_out_ms: u32,
    },
    SetBusVolume {
        bus: AudioBus,
        volume: f32,
        fade_ms: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiRole {
    Window,
    Group,
    Dialog,
    Label,
    Button,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum UiActivation {
    Input(InputAction),
    SelectChoice(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiNode {
    pub id: u64,
    pub role: UiRole,
    pub label: String,
    pub bounds: Rect,
    pub focusable: bool,
    pub focused: bool,
    pub activation: Option<UiActivation>,
    pub children: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiTree {
    pub root: u64,
    pub nodes: BTreeMap<u64, UiNode>,
    pub scale_factor: f32,
    pub safe_area: Rect,
}

impl UiTree {
    #[must_use]
    pub fn digest(&self) -> String {
        stable_digest(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeCommand {
    Save { slot: u32 },
    Load { slot: u32 },
    OpenMenu,
    Quit,
    Unsupported { name: String, arguments: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepOutput {
    pub render: RenderFrame,
    pub audio: Vec<AudioCommand>,
    pub ui: UiTree,
    pub runtime: Vec<RuntimeCommand>,
    pub halted: bool,
}

impl StepOutput {
    #[must_use]
    pub fn digest(&self) -> String {
        stable_digest(self)
    }
}

#[must_use]
pub fn stable_digest<T: Serialize>(value: &T) -> String {
    let encoded =
        serde_json::to_vec(value).expect("serializing an in-memory protocol value cannot fail");
    blake3::hash(&encoded).to_hex().to_string()
}
