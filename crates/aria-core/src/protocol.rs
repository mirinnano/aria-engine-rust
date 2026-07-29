use serde::{Deserialize, Serialize};

use crate::presentation::UiViewModel;
use crate::presentation_state::UiViewport;

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

impl Default for Color {
    fn default() -> Self {
        Self::WHITE
    }
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

/// How a raster sprite occupies its declared destination rectangle.
///
/// This belongs to the frame protocol rather than a renderer preference: a
/// title background must crop in the same way in Native and Web, and an
/// authored UI image must not silently stretch on one target only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpriteFit {
    /// Stretch to the destination. Useful for deliberately procedural assets.
    #[default]
    Fill,
    /// Preserve the source aspect ratio while keeping the whole source visible.
    Contain,
    /// Preserve the source aspect ratio while covering the destination.
    Cover,
}

/// A value-only visual style shared by the native and web renderers.
///
/// The style deliberately contains no renderer handles.  It is therefore part
/// of the deterministic render protocol and can safely be recorded in replay
/// tapes and golden tests.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawStyle {
    pub corner_radius: f32,
    pub opacity: u8,
    pub border: Option<BorderStyle>,
    pub shadow: Option<ShadowStyle>,
    pub gradient: Option<GradientStyle>,
    /// Clips the primitive and all of its visual decoration in logical UI
    /// coordinates.  The renderer uses the same value on Native and Web.
    pub clip: Option<Rect>,
    pub text_align: TextAlign,
    /// Absolute logical-pixel line height. Zero selects the renderer's
    /// deterministic font-size based default.
    pub line_height: f32,
    pub letter_spacing: f32,
    pub text_decoration: TextDecoration,
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self {
            corner_radius: 0.0,
            opacity: 255,
            border: None,
            shadow: None,
            gradient: None,
            clip: None,
            text_align: TextAlign::Start,
            line_height: 0.0,
            letter_spacing: 0.0,
            text_decoration: TextDecoration::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorderStyle {
    pub color: Color,
    pub width: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShadowStyle {
    pub color: Color,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStyle {
    pub start: Color,
    pub end: Color,
    pub angle_degrees: f32,
}

/// Horizontal/vertical alignment for multiline text inside its declared
/// bounds.  The alignment lives in the value protocol, not a platform text
/// adapter, so accessibility and golden tests observe the same layout intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Start,
    Center,
    End,
}

impl Default for TextAlign {
    fn default() -> Self {
        Self::Start
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextDecoration {
    None,
    Shadow {
        color: Color,
        offset_x: i8,
        offset_y: i8,
    },
    Outline {
        color: Color,
        width: u8,
    },
}

impl Default for TextDecoration {
    fn default() -> Self {
        Self::None
    }
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
        #[serde(default = "default_scale", skip_serializing_if = "is_default_scale")]
        scale: f32,
        #[serde(default, skip_serializing_if = "is_default_f32")]
        rotation_degrees: f32,
        #[serde(default, skip_serializing_if = "is_default_color")]
        tint: Color,
        #[serde(default, skip_serializing_if = "is_default_sprite_fit")]
        fit: SpriteFit,
        #[serde(default, skip_serializing_if = "is_default_draw_style")]
        style: DrawStyle,
    },
    Rectangle {
        id: String,
        bounds: Rect,
        color: Color,
        corner_radius: f32,
        z: i32,
        #[serde(default, skip_serializing_if = "is_default_draw_style")]
        style: DrawStyle,
    },
    Text {
        id: String,
        text: String,
        speaker: Option<String>,
        bounds: Rect,
        color: Color,
        font_size: f32,
        z: i32,
        #[serde(default, skip_serializing_if = "is_default_draw_style")]
        style: DrawStyle,
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

fn is_default_draw_style(style: &DrawStyle) -> bool {
    *style == DrawStyle::default()
}

fn default_scale() -> f32 {
    1.0
}

fn is_default_scale(value: &f32) -> bool {
    (*value - 1.0).abs() < f32::EPSILON
}

fn is_default_f32(value: &f32) -> bool {
    value.abs() < f32::EPSILON
}

fn is_default_color(value: &Color) -> bool {
    *value == Color::WHITE
}

fn is_default_sprite_fit(value: &SpriteFit) -> bool {
    *value == SpriteFit::Fill
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Fade,
    FadeThroughBlack,
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScreenEffect {
    Tint {
        color: Color,
        opacity: u8,
        progress: f32,
    },
    Flash {
        color: Color,
        opacity: u8,
        progress: f32,
    },
    Shake {
        amplitude: f32,
        progress: f32,
    },
}

/// Scene-only rendering data. UI layout and interaction never enter this
/// value; a frontend renders [`crate::presentation::UiViewModel`] with its
/// own accessible DOM instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneFrame {
    pub frame_number: u64,
    pub logical_size: LogicalSize,
    #[serde(default)]
    pub viewport: UiViewport,
    pub clear_color: Color,
    pub commands: Vec<DrawCommand>,
    pub transition: Option<TransitionFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<ScreenEffect>,
}

impl SceneFrame {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeCommand {
    Save { slot: u32 },
    Load { slot: u32 },
    ReturnToTitle,
    QuickSave,
    QuickLoad,
    PreloadAsset { asset: String },
    Quit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepOutput {
    pub scene: SceneFrame,
    pub view: UiViewModel,
    pub audio: Vec<AudioCommand>,
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
