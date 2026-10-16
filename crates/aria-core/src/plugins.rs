//! Validated plugin metadata supplied by plugin hosts.
//! The Core consumes data, never a Wasm instance or a host resource.
use std::collections::{BTreeMap, BTreeSet};

pub use aria_plugin_sdk::{
    Capability, ChoiceEdit, CommandDecl, HookEvent, PluginExecutor, PluginManifest,
    PluginRegistration, PluginRequest, PluginResponse, PluginValue, RoutePolicy, StoryState,
    TextEdit, ValueType,
};
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

/// Everything a host hands a VM about its plugins: the validated
/// registrations, and what runs their calls when any plugin needs calls.
#[derive(Debug, Clone, Default)]
pub struct PluginSet {
    pub registrations: Vec<PluginRegistration>,
    pub executor: Option<std::sync::Arc<dyn PluginExecutor>>,
}

#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    routes: BTreeMap<String, RoutePolicy>,
    stamps: BTreeMap<String, PluginStamp>,
    manifests: BTreeMap<String, PluginManifest>,
    /// Script namespace → plugin id.
    namespaces: BTreeMap<String, String>,
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
        if let Some(namespace) = &manifest.namespace
            && let Some(owner) = self.namespaces.get(namespace)
        {
            return Err(PluginRegistryError(format!(
                "namespace '{namespace}' is already used by '{owner}'"
            )));
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
        if let Some(namespace) = &manifest.namespace {
            self.namespaces
                .insert(namespace.clone(), manifest.id.clone());
        }
        self.manifests.insert(manifest.id.clone(), manifest.clone());
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

    #[must_use]
    pub fn manifest(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.manifests.get(plugin_id)
    }

    /// The plugins that subscribe to `event`, in plugin-id order.
    #[must_use]
    pub fn hooked(&self, event: HookEvent) -> Vec<String> {
        self.manifests
            .values()
            .filter(|manifest| manifest.hooks.contains(&event))
            .map(|manifest| manifest.id.clone())
            .collect()
    }

    /// The plugin id behind a script namespace.
    #[must_use]
    pub fn plugin_for_namespace(&self, namespace: &str) -> Option<&str> {
        self.namespaces.get(namespace).map(String::as_str)
    }

    #[must_use]
    pub fn command(&self, plugin_id: &str, name: &str) -> Option<&CommandDecl> {
        self.manifests
            .get(plugin_id)?
            .commands
            .iter()
            .find(|command| command.name == name)
    }

    /// Whether a save made with `saved` plugins can be opened now, and which
    /// plugins must first migrate their state (plugin id, saved version).
    /// Every plugin in the save must be registered, at the same state version
    /// or at a newer one that migrates from the saved version. A plugin added
    /// since the save is fine: it simply has no saved state.
    pub fn plan_restore(
        &self,
        saved: &BTreeMap<String, PluginStamp>,
    ) -> Result<Vec<(String, u32)>, String> {
        let mut migrations = Vec::new();
        for (id, stamp) in saved {
            let Some(current) = self.stamps.get(id) else {
                return Err(format!("the save needs plugin '{id}'"));
            };
            if current.state_version == stamp.state_version {
                continue;
            }
            let migrates_from = self
                .manifests
                .get(id)
                .and_then(|manifest| manifest.migrates_from);
            if stamp.state_version < current.state_version
                && migrates_from.is_some_and(|oldest| oldest <= stamp.state_version)
            {
                migrations.push((id.clone(), stamp.state_version));
                continue;
            }
            return Err(format!(
                "plugin '{id}' keeps state version {}, but the save has version {}",
                current.state_version, stamp.state_version
            ));
        }
        Ok(migrations)
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
                namespace: None,
                commands: Vec::new(),
                migrates_from: None,
                hooks: Default::default(),
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
        assert!(
            registry.plan_restore(&saved).is_ok(),
            "only the code changed"
        );
        saved.get_mut("story").unwrap().state_version = 2;
        assert!(
            registry
                .plan_restore(&saved)
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
        assert!(
            registry
                .plan_restore(&gone)
                .unwrap_err()
                .contains("removed")
        );
        assert!(registry.plan_restore(&BTreeMap::new()).is_ok());
    }

    #[test]
    fn an_older_save_migrates_when_the_plugin_says_it_can() {
        let mut registry = PluginRegistry::default();
        let mut newer = plugin("story", "slide");
        newer.manifest.state_version = 3;
        newer.manifest.migrates_from = Some(2);
        registry.register(newer).unwrap();
        let saved = |state_version| {
            BTreeMap::from([(
                "story".to_owned(),
                PluginStamp {
                    version: "1.0.0".into(),
                    state_version,
                },
            )])
        };
        assert_eq!(
            registry.plan_restore(&saved(2)),
            Ok(vec![("story".to_owned(), 2)])
        );
        assert!(
            registry.plan_restore(&saved(1)).is_err(),
            "too old to migrate"
        );
        assert!(
            registry.plan_restore(&saved(4)).is_err(),
            "from a newer build"
        );
    }
}
