//! Versioned, layout-free contract for Rust-authored Aria Wasm plugins.
//!
//! A plugin declares who it is (`id`, semver `version`), the version of the
//! state it keeps in saves (`state_version`), and the capabilities it uses.
//! It never receives a filesystem, network, or clock. Hosts validate the
//! manifest before installing anything, and no Rust ABI or pointers to host
//! objects cross the boundary.
#![deny(missing_debug_implementations)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub mod calls;
pub use calls::{
    CommandDecl, PluginExecutor, PluginRequest, PluginResponse, PluginValue, StoryState, ValueType,
};

pub const PLUGIN_API_VERSION: u32 = 2;
pub const MAX_PLUGINS: usize = 16;
pub const MAX_ROUTES: usize = 64;
pub const MAX_MANIFEST_BYTES: usize = 65_536;
pub const MAX_COMMANDS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub api_version: u32,
    pub id: String,
    /// Semantic version of this build of the plugin. Games require a range.
    pub version: String,
    /// Version of the state this plugin keeps in saves. A save records it,
    /// and a later build of the plugin opens that save only at the same
    /// state version. Raise it only when the saved state changes shape.
    #[serde(default = "first_state_version")]
    pub state_version: u32,
    /// What the plugin uses. Host functions outside this set are not linked.
    pub capabilities: BTreeSet<Capability>,
    #[serde(default)]
    pub routes: Vec<RoutePolicy>,
    /// The prefix scripts write before a command: `namespace::name(…)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandDecl>,
    /// The oldest state version this build can migrate a save from. Without
    /// it, a save at an older state version cannot be opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrates_from: Option<u32>,
}

fn first_state_version() -> u32 {
    1
}

/// A capability a plugin declares in its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Functions callable from Aria scripts.
    Commands,
    /// Interventions on VM events such as text and choices.
    Hooks,
    /// Input and wait policies for custom screens.
    Routes,
    /// Reading and writing story state, and keeping plugin state in saves.
    State,
    /// Draw commands and, on the Web, a UI module.
    Render,
}

/// Only deterministic policy is contributed; layout and copy remain in the
/// game presentation. Defaults describe an ordinary, typewritten story page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoutePolicy {
    pub route: String,
    pub reading_surface: bool,
    pub atomic_text: bool,
    pub playback_controls: bool,
    pub advance_releases_hold: bool,
    pub skip_releases_hold: bool,
    pub advance_action: Option<String>,
    pub auto_hold_cap_ms: Option<u32>,
    pub first_visit_min_hold_ms: Option<u32>,
}

impl Default for RoutePolicy {
    fn default() -> Self {
        Self {
            route: String::new(),
            reading_surface: true,
            atomic_text: false,
            playback_controls: false,
            advance_releases_hold: false,
            skip_releases_hold: false,
            advance_action: None,
            auto_hold_cap_ms: None,
            first_visit_min_hold_ms: None,
        }
    }
}

impl RoutePolicy {
    #[must_use]
    pub fn reading(route: impl Into<String>) -> Self {
        Self {
            route: route.into(),
            ..Self::default()
        }
    }
}

/// A host binds the validated manifest to the exact guest binary. This hash
/// participates in save compatibility, not just the declared version string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRegistration {
    pub manifest: PluginManifest,
    pub wasm_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid Aria plugin: {0}")]
pub struct PluginContractError(pub String);

impl PluginManifest {
    /// The parsed `version`.
    pub fn semver(&self) -> Result<semver::Version, semver::Error> {
        semver::Version::parse(&self.version)
    }

    pub fn validate(&self) -> Result<(), PluginContractError> {
        let fail = |message: &str| Err(PluginContractError(message.to_owned()));
        if self.api_version != PLUGIN_API_VERSION {
            return fail("unsupported API version (expected 2)");
        }
        if !valid_identifier(&self.id) || self.version.len() > 64 || self.semver().is_err() {
            return fail("id is invalid or version is not a semantic version");
        }
        if self.state_version == 0 {
            return fail("state_version starts at 1");
        }
        if self.capabilities.is_empty() {
            return fail("a plugin must declare at least one capability");
        }
        let declares_routes = self.capabilities.contains(&Capability::Routes);
        if declares_routes == self.routes.is_empty() {
            return fail("routes are declared exactly when the routes capability is");
        }
        if self.routes.len() > MAX_ROUTES {
            return fail("a plugin may declare at most 64 routes");
        }
        let declares_commands = self.capabilities.contains(&Capability::Commands);
        if declares_commands == self.commands.is_empty() {
            return fail("commands are declared exactly when the commands capability is");
        }
        if declares_commands != self.namespace.is_some() {
            return fail("a namespace is declared exactly when commands are");
        }
        if let Some(namespace) = &self.namespace
            && !valid_script_name(namespace)
        {
            return fail("namespace must be a lowercase script identifier");
        }
        if self.commands.len() > MAX_COMMANDS {
            return fail("a plugin may declare at most 64 commands");
        }
        let mut command_names = BTreeSet::new();
        for command in &self.commands {
            if !valid_script_name(&command.name) || !command_names.insert(&command.name) {
                return fail("command name is invalid or duplicated");
            }
            if command.params.len() > 8 {
                return fail("a command takes at most 8 parameters");
            }
        }
        if let Some(from) = self.migrates_from
            && (from == 0 || from >= self.state_version)
        {
            return fail("migrates_from must be below state_version and at least 1");
        }
        let mut routes = BTreeSet::new();
        let mut actions = BTreeSet::new();
        for policy in &self.routes {
            if !valid_identifier(&policy.route) || reserved_route(&policy.route) {
                return fail("route is invalid or reserved by the engine");
            }
            if !routes.insert(&policy.route) {
                return fail("duplicate route");
            }
            if !policy.reading_surface
                && (policy.atomic_text
                    || policy.playback_controls
                    || policy.advance_releases_hold
                    || policy.skip_releases_hold
                    || policy.advance_action.is_some()
                    || policy.auto_hold_cap_ms.is_some()
                    || policy.first_visit_min_hold_ms.is_some())
            {
                return fail("story timing policies require a reading surface");
            }
            if policy.advance_action.is_some() && !policy.advance_releases_hold {
                return fail("an advance action requires a releasable hold");
            }
            if let Some(action) = &policy.advance_action
                && (!valid_identifier(action) || reserved_action(action) || !actions.insert(action))
            {
                return fail("advance action is invalid, reserved, or duplicated");
            }
            for duration in [policy.auto_hold_cap_ms, policy.first_visit_min_hold_ms]
                .into_iter()
                .flatten()
            {
                if duration == 0 || duration > 60_000 {
                    return fail("policy durations must be in 1..=60000 ms");
                }
            }
        }
        Ok(())
    }
}

#[must_use]
pub fn reserved_route(value: &str) -> bool {
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
            | "menu"
            | "rmenu"
            | "config"
            | "log"
            | "chapter"
            | "cg"
    )
}

fn reserved_action(value: &str) -> bool {
    [
        "dialogue.",
        "chrome.",
        "menu.",
        "rmenu.",
        "save.",
        "load.",
        "gallery.",
        "confirm.",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
        || value == "dismiss"
}

/// A name scripts write directly: a lowercase letter, then lowercase
/// letters, digits, or `_`, at most 32 bytes.
fn valid_script_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 32
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

/// Decodes a request, runs `handler`, and encodes its `Result`. Used by
/// `export_plugin!`; plugins do not call it directly.
#[doc(hidden)]
pub fn dispatch(
    input: &[u8],
    handler: fn(PluginRequest) -> Result<PluginResponse, String>,
) -> Vec<u8> {
    let outcome = serde_json::from_slice::<PluginRequest>(input)
        .map_err(|error| format!("malformed request: {error}"))
        .and_then(handler);
    serde_json::to_vec(&outcome).unwrap_or_else(|_| br#"{"Err":"unencodable response"}"#.to_vec())
}

#[doc(hidden)]
pub fn manifest_bytes(manifest: &PluginManifest) -> Vec<u8> {
    serde_json::to_vec(manifest).expect("plugin manifest contains only serializable values")
}

/// Exports API v1 from a `cdylib` built for `wasm32-unknown-unknown`.
/// `$factory` is a function returning a `PluginManifest`. The resulting bytes
/// remain guest-owned and immutable. The host reads bounded linear memory;
/// it never dereferences a guest pointer in the native process.
#[macro_export]
macro_rules! export_plugin {
    ($factory:path, $handler:path) => {
        $crate::export_plugin!($factory);
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn aria_alloc(len: u32) -> u32 {
            let mut buffer = ::std::vec::Vec::<u8>::with_capacity(len as usize);
            let pointer = buffer.as_mut_ptr();
            ::std::mem::forget(buffer);
            pointer as u32
        }
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn aria_call(pointer: u32, len: u32) -> u64 {
            // SAFETY: the host wrote `len` bytes at a pointer from `aria_alloc`.
            let input = unsafe { ::std::slice::from_raw_parts(pointer as *const u8, len as usize) };
            let output = $crate::dispatch(input, $handler);
            let result = ((output.as_ptr() as u64) << 32) | output.len() as u64;
            ::std::mem::forget(output);
            result
        }
    };
    ($factory:path) => {
        #[cfg(target_arch = "wasm32")]
        fn __aria_plugin_manifest() -> &'static [u8] {
            static BYTES: ::std::sync::OnceLock<::std::vec::Vec<u8>> = ::std::sync::OnceLock::new();
            BYTES
                .get_or_init(|| $crate::manifest_bytes(&$factory()))
                .as_slice()
        }
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn aria_plugin_api_version() -> u32 {
            $crate::PLUGIN_API_VERSION
        }
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn aria_plugin_manifest_ptr() -> u32 {
            __aria_plugin_manifest().as_ptr() as u32
        }
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn aria_plugin_manifest_len() -> u32 {
            __aria_plugin_manifest().len() as u32
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> PluginManifest {
        PluginManifest {
            api_version: 2,
            id: "example.lecture".into(),
            version: "1.0.0".into(),
            state_version: 1,
            capabilities: [Capability::Routes].into(),
            routes: vec![RoutePolicy::reading("lecture")],
            namespace: None,
            commands: Vec::new(),
            migrates_from: None,
        }
    }

    #[test]
    fn commands_need_a_namespace_and_valid_names() {
        let mut plugin = manifest();
        plugin.capabilities.insert(Capability::Commands);
        plugin.commands.push(CommandDecl {
            name: "score".into(),
            params: vec![ValueType::Int],
            returns: Some(ValueType::Int),
        });
        assert!(plugin.validate().is_err(), "no namespace");
        plugin.namespace = Some("story".into());
        assert!(plugin.validate().is_ok());
        plugin.namespace = Some("Story".into());
        assert!(plugin.validate().is_err());
        plugin.namespace = Some("story".into());
        plugin.commands.push(plugin.commands[0].clone());
        assert!(plugin.validate().is_err(), "duplicate command");
        plugin.commands.pop();
        plugin.migrates_from = Some(1);
        assert!(plugin.validate().is_err(), "nothing older to migrate from");
        plugin.state_version = 2;
        assert!(plugin.validate().is_ok());
    }

    #[test]
    fn values_and_responses_have_a_stable_json_form() {
        let request = PluginRequest::Command {
            command: "score".into(),
            args: vec![
                PluginValue::Int(3),
                PluginValue::Bool(true),
                PluginValue::String("a".into()),
            ],
            state: serde_json::Value::Null,
            story: StoryState::default(),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(
            json.starts_with(r#"{"kind":"command","command":"score","args":[3,true,"a"]"#),
            "{json}"
        );
        assert_eq!(
            serde_json::from_str::<PluginRequest>(&json).unwrap(),
            request
        );
        let encoded = dispatch(json.as_bytes(), |_| {
            Ok(PluginResponse {
                result: Some(PluginValue::Int(7)),
                ..PluginResponse::default()
            })
        });
        let decoded: Result<PluginResponse, String> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.unwrap().result, Some(PluginValue::Int(7)));
        let failed: Result<PluginResponse, String> =
            serde_json::from_slice(&dispatch(b"{}", |_| unreachable!())).unwrap();
        assert!(failed.unwrap_err().contains("malformed"));
    }
    #[test]
    fn contract_rejects_reserved_and_duplicate_routes_and_actions() {
        let mut plugin = manifest();
        assert!(plugin.validate().is_ok());
        plugin.routes[0].route = "config".into();
        assert!(plugin.validate().is_err());
        plugin = manifest();
        plugin.routes.push(plugin.routes[0].clone());
        assert!(plugin.validate().is_err());
        plugin = manifest();
        plugin.routes[0].advance_releases_hold = true;
        plugin.routes[0].advance_action = Some("menu.quit".into());
        assert!(plugin.validate().is_err());
        plugin.routes[0].advance_action = Some("rmenu.close".into());
        assert!(plugin.validate().is_err());
    }
    #[test]
    fn capabilities_state_version_and_routes_must_agree() {
        let mut plugin = manifest();
        plugin.capabilities.clear();
        assert!(plugin.validate().is_err());
        plugin = manifest();
        plugin.state_version = 0;
        assert!(plugin.validate().is_err());
        plugin = manifest();
        plugin.capabilities = [Capability::State].into();
        assert!(plugin.validate().is_err(), "routes without the capability");
        plugin.routes.clear();
        assert!(plugin.validate().is_ok());
        let parsed: PluginManifest = serde_json::from_str(
            r#"{"api_version":2,"id":"x","version":"0.3.1","capabilities":["state"]}"#,
        )
        .unwrap();
        assert_eq!(parsed.state_version, 1);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn unknown_fields_and_new_api_are_not_silently_accepted() {
        let mut plugin = manifest();
        plugin.api_version = 1;
        assert!(plugin.validate().is_err());
        plugin = manifest();
        plugin.version = "1\u{001b}[2J".into();
        assert!(plugin.validate().is_err());
        plugin = manifest();
        plugin.version = "1".into();
        assert!(plugin.validate().is_err(), "a version must be semver");
        assert!(
            serde_json::from_str::<PluginManifest>(
                r#"{"api_version":2,"id":"x","version":"1.0.0","capabilities":["routes"],"routes":[],"filesystem":true}"#
            )
            .is_err()
        );
    }
}
