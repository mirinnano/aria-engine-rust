//! What a running VM sends a plugin, and what the plugin answers.
//!
//! A call is one JSON request in and one JSON response out. The guest gets
//! no host functions: everything it may read arrives in the request, and
//! everything it changes leaves in the response. The host creates a fresh
//! Wasm instance for every call, so a call cannot see leftovers from an
//! earlier one and a replay of the same requests gives the same answers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An Aria value crossing the plugin boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PluginValue {
    Bool(bool),
    Int(i64),
    String(String),
}

impl PluginValue {
    #[must_use]
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::Int(_) => ValueType::Int,
            Self::String(_) => ValueType::String,
        }
    }
}

/// The type of a command parameter or result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Int,
    Bool,
    String,
}

/// A command the plugin offers to scripts as `namespace::name(…)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDecl {
    pub name: String,
    #[serde(default)]
    pub params: Vec<ValueType>,
    /// `None` for a command used only as a statement.
    #[serde(default)]
    pub returns: Option<ValueType>,
}

/// Story state a plugin with the `state` capability can read. Variables are
/// the script's `state` declarations (Bool values appear as 0 or 1); flags
/// are the session flags.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryState {
    #[serde(default)]
    pub vars: BTreeMap<String, PluginValue>,
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginRequest {
    /// A script called `namespace::command(args)`.
    Command {
        command: String,
        args: Vec<PluginValue>,
        /// The plugin's own saved state, `null` before its first write.
        state: serde_json::Value,
        story: StoryState,
    },
    /// A save made at `from` state version is being opened.
    Migrate { from: u32, state: serde_json::Value },
    /// A line is about to be shown. `text` has its `{variables}` filled in.
    Text {
        scene: String,
        speaker: Option<String>,
        text: String,
        state: serde_json::Value,
        story: StoryState,
    },
    /// A choice is about to be offered, with these option labels in order.
    Choices {
        scene: String,
        options: Vec<String>,
        state: serde_json::Value,
        story: StoryState,
    },
    /// The story entered `scene`.
    SceneEnter {
        scene: String,
        state: serde_json::Value,
        story: StoryState,
    },
}

/// A VM event a plugin can intervene in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    Text,
    Choices,
    SceneEnter,
}

/// A replacement for the line about to be shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextEdit {
    /// The speaker name to show; `None` shows the line as narration.
    #[serde(default)]
    pub speaker: Option<String>,
    pub text: String,
}

/// One option of the edited choice, in the order to offer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceEdit {
    /// Keep the original option at this index (0-based).
    #[serde(default)]
    pub from: Option<u32>,
    /// A new label; required for an added option.
    #[serde(default)]
    pub label: Option<String>,
    /// For an added option (`from` absent): the scene it leads to.
    #[serde(default)]
    pub scene: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginResponse {
    /// The command's result, when it declares one.
    #[serde(default)]
    pub result: Option<PluginValue>,
    /// The plugin's new saved state. Absent leaves it unchanged.
    #[serde(default)]
    pub state: Option<serde_json::Value>,
    /// Story variables and flags to change. Requires the `state` capability.
    #[serde(default)]
    pub story: StoryState,
    /// For a `text` event: the line to show instead. Absent keeps it.
    #[serde(default)]
    pub text: Option<TextEdit>,
    /// For a `choices` event: the options to offer instead. Absent keeps them.
    #[serde(default)]
    pub choices: Option<Vec<ChoiceEdit>>,
    /// The plugin's whole draw layer, replacing the previous one; an empty
    /// list clears it. Requires the `render` capability.
    #[serde(default)]
    pub draw: Option<Vec<PluginDraw>>,
}

/// One primitive in a plugin's draw layer, in logical pixels. Colors are
/// `#rrggbb` or `#rrggbbaa`; images name a game asset by logical path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginDraw {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: String,
        #[serde(default)]
        corner_radius: f32,
        #[serde(default)]
        z: i32,
    },
    Image {
        asset: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        #[serde(default = "opaque")]
        opacity: u8,
        #[serde(default)]
        z: i32,
    },
    Text {
        text: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        font_size: f32,
        color: String,
        #[serde(default)]
        z: i32,
    },
}

fn opaque() -> u8 {
    255
}

/// At most this many primitives per plugin layer.
pub const MAX_DRAW_COMMANDS: usize = 256;

/// Runs plugin calls for a VM. The Core holds one of these, never a Wasm
/// instance; `aria-plugin-host` provides the Wasm implementation.
pub trait PluginExecutor: std::fmt::Debug + Send + Sync {
    fn call(&self, plugin_id: &str, request: &PluginRequest) -> Result<PluginResponse, String>;
}
