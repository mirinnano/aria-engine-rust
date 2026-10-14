//! An independent plugin: a skippable complete slide, no Umikaze route names.
use aria_plugin_sdk::{PLUGIN_API_VERSION, PluginManifest, RoutePolicy};

#[must_use]
pub fn manifest() -> PluginManifest {
    PluginManifest {
        api_version: PLUGIN_API_VERSION,
        id: "org.example.lecture".into(),
        version: "1.0.0".into(),
        routes: vec![RoutePolicy {
            atomic_text: true,
            advance_releases_hold: true,
            advance_action: Some("lecture.next".into()),
            ..RoutePolicy::reading("lecture_slide")
        }],
    }
}

aria_plugin_sdk::export_plugin!(manifest);

#[cfg(test)]
mod tests {
    #[test]
    fn independent_contract_is_valid() {
        super::manifest().validate().unwrap();
    }
}
