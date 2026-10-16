//! A sample plugin with commands and state: a running total that scripts
//! add to, saved with the game, plus a command that marks story state.
//!
//! ```aria
//! let total = tally::add(3);
//! if tally::reached(10) { … }
//! tally::mark("shore");
//! ```
use aria_plugin_sdk::{
    Capability, CommandDecl, PLUGIN_API_VERSION, PluginManifest, PluginRequest, PluginResponse,
    PluginValue, ValueType,
};
use serde_json::{Value, json};

#[must_use]
pub fn manifest() -> PluginManifest {
    PluginManifest {
        api_version: PLUGIN_API_VERSION,
        id: "org.example.tally".into(),
        version: "1.0.0".into(),
        // Version 1 saved the total as {"sum": n}; version 2 as {"total": n}.
        state_version: 2,
        migrates_from: Some(1),
        hooks: Default::default(),
        capabilities: [Capability::Commands, Capability::State].into(),
        routes: Vec::new(),
        namespace: Some("tally".into()),
        commands: vec![
            CommandDecl {
                name: "add".into(),
                params: vec![ValueType::Int],
                returns: Some(ValueType::Int),
            },
            CommandDecl {
                name: "reached".into(),
                params: vec![ValueType::Int],
                returns: Some(ValueType::Bool),
            },
            CommandDecl {
                name: "mark".into(),
                params: vec![ValueType::String],
                returns: None,
            },
        ],
    }
}

/// Answers every call. Pure: the answer depends only on the request.
pub fn handle(request: PluginRequest) -> Result<PluginResponse, String> {
    match request {
        PluginRequest::Migrate { from: 1, state } => Ok(PluginResponse {
            state: Some(json!({ "total": state.get("sum").and_then(Value::as_i64).unwrap_or(0) })),
            ..PluginResponse::default()
        }),
        PluginRequest::Migrate { from, .. } => {
            Err(format!("cannot migrate from state version {from}"))
        }
        // Tally does not subscribe to any VM event.
        PluginRequest::Text { .. }
        | PluginRequest::Choices { .. }
        | PluginRequest::SceneEnter { .. } => Err("tally has no hooks".into()),
        PluginRequest::Command {
            command,
            args,
            state,
            story,
        } => {
            let total = state.get("total").and_then(Value::as_i64).unwrap_or(0);
            match (command.as_str(), args.as_slice()) {
                ("add", [PluginValue::Int(amount)]) => {
                    let total = total.saturating_add(*amount);
                    Ok(PluginResponse {
                        result: Some(PluginValue::Int(total)),
                        state: Some(json!({ "total": total })),
                        ..PluginResponse::default()
                    })
                }
                ("reached", [PluginValue::Int(goal)]) => Ok(PluginResponse {
                    result: Some(PluginValue::Bool(total >= *goal)),
                    ..PluginResponse::default()
                }),
                ("mark", [PluginValue::String(place)]) => {
                    let mut response = PluginResponse::default();
                    response
                        .story
                        .flags
                        .insert(format!("visited_{place}"), true);
                    if let Some(PluginValue::Int(visits)) = story.vars.get("visits") {
                        response
                            .story
                            .vars
                            .insert("visits".into(), PluginValue::Int(visits + 1));
                    }
                    Ok(response)
                }
                _ => Err(format!("unknown call {command}")),
            }
        }
    }
}

aria_plugin_sdk::export_plugin!(manifest, handle);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_contract_is_valid() {
        manifest().validate().unwrap();
    }
}
