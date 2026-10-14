//! Umikaze's reading semantics, not engine-standard route names.
use aria_plugin_sdk::{PLUGIN_API_VERSION, PluginManifest, RoutePolicy};

#[must_use]
pub fn manifest() -> PluginManifest {
    PluginManifest {
        api_version: PLUGIN_API_VERSION,
        id: "jp.umikaze.story".into(),
        version: "1.0.0".into(),
        routes: vec![
            RoutePolicy::reading("day_card"),
            RoutePolicy {
                atomic_text: true,
                playback_controls: true,
                advance_releases_hold: true,
                skip_releases_hold: true,
                advance_action: Some("interlude.advance".into()),
                auto_hold_cap_ms: Some(320),
                first_visit_min_hold_ms: Some(600),
                ..RoutePolicy::reading("interlude")
            },
            RoutePolicy {
                atomic_text: true,
                playback_controls: true,
                skip_releases_hold: true,
                ..RoutePolicy::reading("statement")
            },
        ],
    }
}

aria_plugin_sdk::export_plugin!(manifest);

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_matches_the_public_contract() {
        super::manifest().validate().unwrap();
    }
}
