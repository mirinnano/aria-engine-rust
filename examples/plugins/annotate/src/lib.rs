//! A sample plugin with hooks: it rewrites lines, edits choices, and counts
//! scene visits, which it also draws in a corner, without the script
//! calling it.
use aria_plugin_sdk::{
    Capability, ChoiceEdit, HookEvent, PLUGIN_API_VERSION, PluginDraw, PluginManifest,
    PluginRequest, PluginResponse, PluginValue, TextEdit,
};
use serde_json::{Value, json};

#[must_use]
pub fn manifest() -> PluginManifest {
    PluginManifest {
        api_version: PLUGIN_API_VERSION,
        id: "org.example.annotate".into(),
        version: "1.0.0".into(),
        state_version: 1,
        migrates_from: None,
        capabilities: [Capability::Hooks, Capability::State, Capability::Render].into(),
        hooks: [HookEvent::Text, HookEvent::Choices, HookEvent::SceneEnter].into(),
        routes: Vec::new(),
        namespace: None,
        commands: Vec::new(),
    }
}

pub fn handle(request: PluginRequest) -> Result<PluginResponse, String> {
    match request {
        // Gives 海 a reading wherever it is not already annotated.
        PluginRequest::Text { speaker, text, .. } => Ok(PluginResponse {
            text: (text.contains('海') && !text.contains('《')).then(|| TextEdit {
                speaker,
                text: text.replace('海', "|海《うみ》"),
            }),
            ..PluginResponse::default()
        }),
        // Hides the locked door until the key is found; adds a way back.
        PluginRequest::Choices { options, story, .. } => {
            let has_key = story.flags.get("has_key").copied().unwrap_or(false);
            let mut edits = options
                .iter()
                .enumerate()
                .filter(|(_, label)| has_key || label.as_str() != "鍵のない扉")
                .map(|(index, _)| ChoiceEdit {
                    from: Some(index as u32),
                    label: None,
                    scene: None,
                })
                .collect::<Vec<_>>();
            edits.push(ChoiceEdit {
                from: None,
                label: Some("最初から".into()),
                scene: Some("start".into()),
            });
            Ok(PluginResponse {
                choices: Some(edits),
                ..PluginResponse::default()
            })
        }
        // Counts every scene entered and mirrors the count into `visits`.
        PluginRequest::SceneEnter { state, story, .. } => {
            let count = state.get("scenes").and_then(Value::as_i64).unwrap_or(0) + 1;
            let mut response = PluginResponse {
                state: Some(json!({ "scenes": count })),
                draw: Some(vec![
                    PluginDraw::Rect {
                        x: 1180.0,
                        y: 24.0,
                        width: 76.0,
                        height: 40.0,
                        color: "#05070bcc".into(),
                        corner_radius: 6.0,
                        z: 500,
                    },
                    PluginDraw::Text {
                        text: count.to_string(),
                        x: 1180.0,
                        y: 30.0,
                        width: 76.0,
                        height: 28.0,
                        font_size: 20.0,
                        color: "#ffffff".into(),
                        z: 501,
                    },
                ]),
                ..PluginResponse::default()
            };
            if story.vars.contains_key("visits") {
                response
                    .story
                    .vars
                    .insert("visits".into(), PluginValue::Int(count));
            }
            Ok(response)
        }
        PluginRequest::Command { .. } | PluginRequest::Migrate { .. } => {
            Err("annotate only has hooks".into())
        }
    }
}

aria_plugin_sdk::export_plugin!(manifest, handle);

#[cfg(test)]
mod tests {
    #[test]
    fn the_contract_is_valid() {
        super::manifest().validate().unwrap();
    }
}
