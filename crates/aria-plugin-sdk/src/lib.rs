//! Versioned, layout-free contract for Rust-authored Aria Wasm plugins.
//!
//! API v1 runs guest Rust code at registration to describe custom story
//! surfaces. It does not grant a filesystem, network, clock, or VM mutation.
//! Hosts validate the result before installing any policy. No Rust ABI or
//! pointers to host objects cross the boundary.
#![deny(missing_debug_implementations)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const PLUGIN_API_VERSION: u32 = 1;
pub const MAX_PLUGINS: usize = 16;
pub const MAX_ROUTES: usize = 64;
pub const MAX_MANIFEST_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub api_version: u32,
    pub id: String,
    pub version: String,
    pub routes: Vec<RoutePolicy>,
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
    pub fn validate(&self) -> Result<(), PluginContractError> {
        let fail = |message: &str| Err(PluginContractError(message.to_owned()));
        if self.api_version != PLUGIN_API_VERSION {
            return fail("unsupported API version (expected 1)");
        }
        if !valid_identifier(&self.id)
            || self.version.is_empty()
            || self.version.len() > 64
            || !self.version.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
            })
        {
            return fail("id or version is invalid");
        }
        if self.routes.is_empty() || self.routes.len() > MAX_ROUTES {
            return fail("a plugin must declare 1..=64 routes");
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

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
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
            api_version: 1,
            id: "example.lecture".into(),
            version: "1.0.0".into(),
            routes: vec![RoutePolicy::reading("lecture")],
        }
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
    fn unknown_fields_and_new_api_are_not_silently_accepted() {
        let mut plugin = manifest();
        plugin.api_version = 2;
        assert!(plugin.validate().is_err());
        plugin = manifest();
        plugin.version = "1\u{001b}[2J".into();
        assert!(plugin.validate().is_err());
        assert!(
            serde_json::from_str::<PluginManifest>(
                r#"{"api_version":1,"id":"x","version":"1","routes":[],"filesystem":true}"#
            )
            .is_err()
        );
    }
}
