//! Immutable, validated story-surface policies supplied by plugin hosts.
//! The Core consumes data, never a Wasm instance or a host resource.
use std::collections::{BTreeMap, BTreeSet};

use aria_plugin_sdk::{MAX_PLUGINS, MAX_ROUTES};
pub use aria_plugin_sdk::{PluginManifest, PluginRegistration, RoutePolicy};
use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    routes: BTreeMap<String, RoutePolicy>,
    fingerprints: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("plugin registration rejected: {0}")]
pub struct PluginRegistryError(pub String);

impl PluginRegistry {
    pub fn register(&mut self, plugin: PluginRegistration) -> Result<(), PluginRegistryError> {
        plugin
            .manifest
            .validate()
            .map_err(|error| PluginRegistryError(error.to_string()))?;
        let manifest = plugin.manifest;
        if self.fingerprints.len() >= MAX_PLUGINS
            || self.routes.len() + manifest.routes.len() > MAX_ROUTES
        {
            return Err(PluginRegistryError(
                "plugin or route count exceeds the limit".into(),
            ));
        }
        if self.fingerprints.contains_key(&manifest.id) {
            return Err(PluginRegistryError(format!(
                "duplicate plugin '{}'",
                manifest.id
            )));
        }
        if plugin.wasm_blake3.len() != 64
            || !plugin
                .wasm_blake3
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(PluginRegistryError(
                "invalid guest binary fingerprint".into(),
            ));
        }
        let mut actions = self
            .routes
            .values()
            .filter_map(|policy| policy.advance_action.as_deref())
            .collect::<BTreeSet<_>>();
        for policy in &manifest.routes {
            if self.routes.contains_key(&policy.route) {
                return Err(PluginRegistryError(format!(
                    "route '{}' is already registered",
                    policy.route
                )));
            }
            if let Some(action) = &policy.advance_action
                && !actions.insert(action)
            {
                return Err(PluginRegistryError(format!(
                    "action '{action}' is already registered"
                )));
            }
        }
        // Also bind the validated policy data. Host callers cannot use the same
        // declared binary hash with different metadata to resume another policy.
        let metadata = aria_plugin_sdk::manifest_bytes(&manifest);
        let mut fingerprint = blake3::Hasher::new_derive_key("Aria plugin registration v1");
        fingerprint.update(plugin.wasm_blake3.as_bytes());
        fingerprint.update(&metadata);
        self.fingerprints
            .insert(manifest.id, fingerprint.finalize().to_hex().to_string());
        for policy in manifest.routes {
            self.routes.insert(policy.route.clone(), policy);
        }
        Ok(())
    }

    #[must_use]
    pub fn policy(&self, route: &str) -> Option<&RoutePolicy> {
        self.routes.get(route)
    }
    #[must_use]
    pub fn fingerprints(&self) -> &BTreeMap<String, String> {
        &self.fingerprints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plugin(id: &str, route: &str) -> PluginRegistration {
        PluginRegistration {
            manifest: PluginManifest {
                api_version: 1,
                id: id.into(),
                version: "1".into(),
                routes: vec![RoutePolicy::reading(route)],
            },
            wasm_blake3: "a".repeat(64),
        }
    }
    #[test]
    fn failed_registration_is_atomic_and_collisions_are_rejected() {
        let mut registry = PluginRegistry::default();
        registry.register(plugin("one", "slide")).unwrap();
        let before = registry.fingerprints().clone();
        assert!(registry.register(plugin("two", "slide")).is_err());
        assert_eq!(registry.fingerprints(), &before);
        assert!(registry.register(plugin("one", "another")).is_err());
        assert!(registry.policy("another").is_none());
    }
}
