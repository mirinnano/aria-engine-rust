use serde::{Deserialize, Serialize};

/// Result of a pure source migration. Filesystem backup and replacement are
/// intentionally the CLI adapter's responsibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptMigration {
    pub source: String,
    pub changed: bool,
    pub notices: Vec<String>,
}

#[must_use]
pub fn migrate_script_to_v3(source: &str) -> ScriptMigration {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_final_newline = source.ends_with('\n');
    let mut lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut changed = false;
    let mut notices = Vec::new();
    let version_line = lines.iter().position(|line| {
        let trimmed = line.trim().to_ascii_lowercase();
        trimmed.starts_with("# aria-version:") || trimmed.starts_with("#aria-version:")
    });
    match version_line {
        Some(index) if !lines[index].contains("3.0") => {
            notices.push(format!(
                "updated language header from '{}'",
                lines[index].trim()
            ));
            lines[index] = "# aria-version: 3.0".to_owned();
            changed = true;
        }
        Some(_) => {}
        None => {
            lines.insert(0, "# aria-version: 3.0".to_owned());
            notices.push("inserted the V3 language header".to_owned());
            changed = true;
        }
    }

    let strict_line = lines.iter().position(|line| {
        let trimmed = line.trim().to_ascii_lowercase();
        trimmed == "strict on" || trimmed == "strict off"
    });
    match strict_line {
        Some(index) if lines[index].trim().eq_ignore_ascii_case("strict off") => {
            lines[index] = "strict on".to_owned();
            notices.push("enabled strict mode".to_owned());
            changed = true;
        }
        Some(_) => {}
        None => {
            let insert_at = lines
                .iter()
                .position(|line| line.trim().to_ascii_lowercase().contains("aria-version"))
                .map_or(0, |index| index + 1);
            lines.insert(insert_at, "strict on".to_owned());
            notices.push("enabled strict mode".to_owned());
            changed = true;
        }
    }

    for line in &mut lines {
        let leading_len = line.len() - line.trim_start().len();
        let (leading, body) = line.split_at(leading_len);
        if let Some(normalized) = normalize_atomic_legacy_assignment(body) {
            *line = format!("{leading}{normalized}");
            changed = true;
            notices.push("normalized legacy '=' assignment syntax".to_owned());
            continue;
        }
        let replacements = [
            ("playbgm ", "play_bgm "),
            ("stopbgm", "stop_bgm"),
            ("playse ", "play_se "),
            ("stopse", "stop_se"),
        ];
        for (legacy, v3) in replacements {
            if body.to_ascii_lowercase().starts_with(legacy) {
                *line = format!("{leading}{v3}{}", &body[legacy.len()..]);
                changed = true;
                notices.push(format!("renamed '{legacy}' to '{v3}'"));
                break;
            }
        }
    }

    notices.sort();
    notices.dedup();
    let mut migrated = lines.join(newline);
    if had_final_newline {
        migrated.push_str(newline);
    }
    ScriptMigration {
        changed,
        source: migrated,
        notices,
    }
}

fn normalize_atomic_legacy_assignment(body: &str) -> Option<String> {
    let (command, rest) = body.split_once(char::is_whitespace)?;
    if !matches!(command.to_ascii_lowercase().as_str(), "let" | "mov") || rest.contains(',') {
        return None;
    }
    let (target, value) = rest.split_once(" = ")?;
    let target = target.trim();
    let value = value.trim();
    if !(target.starts_with('%') || target.starts_with('$')) || !atomic_assignment_value(value) {
        return None;
    }
    Some(format!("{command} {target}, {value}"))
}

fn atomic_assignment_value(value: &str) -> bool {
    let quoted = value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')));
    quoted
        || value.strip_prefix(['%', '$']).is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        })
        || value.parse::<i64>().is_ok()
        || value.parse::<f64>().is_ok()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "false" | "on" | "off"
        )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserConfigV3 {
    pub schema_version: u32,
    pub text_speed_ms: u32,
    pub bgm_volume: f32,
    pub sound_effect_volume: f32,
    pub fullscreen: bool,
    pub language: String,
    pub auto_advance_ms: u32,
    pub text_scale: f32,
    pub high_contrast: bool,
}

impl Default for UserConfigV3 {
    fn default() -> Self {
        Self {
            schema_version: 3,
            text_speed_ms: 30,
            bgm_volume: 1.0,
            sound_effect_volume: 1.0,
            fullscreen: false,
            language: "ja-JP".to_owned(),
            auto_advance_ms: 2_000,
            text_scale: 1.0,
            high_contrast: false,
        }
    }
}

#[must_use]
pub fn migrate_legacy_config(value: &serde_json::Value) -> UserConfigV3 {
    let mut config = UserConfigV3::default();
    config.text_speed_ms = json_u32(value, &["GlobalTextSpeedMs", "globalTextSpeedMs"])
        .unwrap_or(config.text_speed_ms);
    config.bgm_volume = json_u32(value, &["BgmVolume", "bgmVolume"])
        .map(percent)
        .unwrap_or(config.bgm_volume);
    config.sound_effect_volume = json_u32(value, &["SeVolume", "seVolume"])
        .map(percent)
        .unwrap_or(config.sound_effect_volume);
    config.fullscreen =
        json_bool(value, &["IsFullscreen", "isFullscreen"]).unwrap_or(config.fullscreen);
    config.language =
        json_string(value, &["Language", "language"]).unwrap_or_else(|| config.language.clone());
    config.auto_advance_ms = json_u32(value, &["AutoModeWaitTimeMs", "autoModeWaitTimeMs"])
        .unwrap_or(config.auto_advance_ms);
    config
}

fn json_u32(value: &serde_json::Value, names: &[&str]) -> Option<u32> {
    names
        .iter()
        .find_map(|name| value.get(*name))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn json_bool(value: &serde_json::Value, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| value.get(*name))
        .and_then(serde_json::Value::as_bool)
}

fn json_string(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn percent(value: u32) -> f32 {
    (value.min(100) as f32) / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_migration_is_idempotent() {
        let first = migrate_script_to_v3(
            "# aria-version: 2.0\nlet %route = 1\nlet %sum = %route + 2\nplaybgm \"sea.ogg\"\n",
        );
        assert!(first.changed);
        assert!(first.source.starts_with("# aria-version: 3.0\nstrict on\n"));
        assert!(first.source.contains("let %route, 1"));
        assert!(first.source.contains("let %sum = %route + 2"));
        let second = migrate_script_to_v3(&first.source);
        assert!(!second.changed, "{}", second.source);
        assert_eq!(second.source, first.source);
    }

    #[test]
    fn legacy_config_percentages_are_normalized() {
        let legacy = serde_json::json!({
            "BgmVolume": 75,
            "SeVolume": 20,
            "Language": "en-US"
        });
        let config = migrate_legacy_config(&legacy);
        assert_eq!(config.bgm_volume, 0.75);
        assert_eq!(config.sound_effect_volume, 0.2);
        assert_eq!(config.language, "en-US");
    }
}
