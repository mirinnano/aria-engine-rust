//! Validated plugin metadata supplied by plugin hosts.
//! The Core consumes data, never a Wasm instance or a host resource.
use std::collections::{BTreeMap, BTreeSet};

pub use aria_plugin_sdk::{Capability, PluginManifest, PluginRegistration, RoutePolicy};
use aria_plugin_sdk::{MAX_PLUGINS, MAX_ROUTES};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What a save records about one plugin. A later build of the plugin opens
/// the save when its `state_version` is the same; `version` is informative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginStamp {
    pub version: String,
    pub state_version: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    routes: BTreeMap<String, RoutePolicy>,
    stamps: BTreeMap<String, PluginStamp>,
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
        if self.stamps.len() >= MAX_PLUGINS
            || self.routes.len() + manifest.routes.len() > MAX_ROUTES
        {
            return Err(PluginRegistryError(
                "plugin or route count exceeds the limit".into(),
            ));
        }
        if self.stamps.contains_key(&manifest.id) {
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
        self.stamps.insert(
            manifest.id.clone(),
            PluginStamp {
                version: manifest.version.clone(),
                state_version: manifest.state_version,
            },
        );
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
    pub fn stamps(&self) -> &BTreeMap<String, PluginStamp> {
        &self.stamps
    }

    /// Whether a save made with `saved` plugins can be opened now. Every
    /// plugin in the save must be registered at the same state version.
    /// A plugin added since the save is fine: it simply has no saved state.
    pub fn accepts(&self, saved: &BTreeMap<String, PluginStamp>) -> Result<(), String> {
        for (id, stamp) in saved {
            match self.stamps.get(id) {
                None => return Err(format!("the save needs plugin '{id}'")),
                Some(current) if current.state_version != stamp.state_version => {
                    return Err(format!(
                        "plugin '{id}' keeps state version {}, but the save has version {}",
                        current.state_version, stamp.state_version
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plugin(id: &str, route: &str) -> PluginRegistration {
        PluginRegistration {
            manifest: PluginManifest {
                api_version: 2,
                id: id.into(),
                version: "1.0.0".into(),
                state_version: 1,
                capabilities: [Capability::Routes].into(),
                routes: vec![RoutePolicy::reading(route)],
            },
            wasm_blake3: "a".repeat(64),
        }
    }
    #[test]
    fn failed_registration_is_atomic_and_collisions_are_rejected() {
        let mut registry = PluginRegistry::default();
        registry.register(plugin("one", "slide")).unwrap();
        let before = registry.stamps().clone();
        assert!(registry.register(plugin("two", "slide")).is_err());
        assert_eq!(registry.stamps(), &before);
        assert!(registry.register(plugin("one", "another")).is_err());
        assert!(registry.policy("another").is_none());
    }

    #[test]
    fn a_save_opens_while_the_state_version_holds() {
        let mut registry = PluginRegistry::default();
        registry.register(plugin("story", "slide")).unwrap();
        let mut saved = registry.stamps().clone();
        saved.get_mut("story").unwrap().version = "0.9.0".into();
        assert!(registry.accepts(&saved).is_ok(), "only the code changed");
        saved.get_mut("story").unwrap().state_version = 2;
        assert!(
            registry
                .accepts(&saved)
                .unwrap_err()
                .contains("state version")
        );
        let gone = BTreeMap::from([(
            "removed".to_owned(),
            PluginStamp {
                version: "1.0.0".into(),
                state_version: 1,
            },
        )]);
        assert!(registry.accepts(&gone).unwrap_err().contains("removed"));
        assert!(registry.accepts(&BTreeMap::new()).is_ok());
    }
}
