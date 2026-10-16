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
}

/// Runs plugin calls for a VM. The Core holds one of these, never a Wasm
/// instance; `aria-plugin-host` provides the Wasm implementation.
pub trait PluginExecutor: std::fmt::Debug + Send + Sync {
    fn call(&self, plugin_id: &str, request: &PluginRequest) -> Result<PluginResponse, String>;
}
