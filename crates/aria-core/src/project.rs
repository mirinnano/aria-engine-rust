use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Schema 4 makes a game-owned presentation package explicit. Aria no longer
/// embeds visual UI declarations in story source or ARIAC.
pub const PROJECT_SCHEMA: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub schema: u32,
    pub game: GameManifest,
    pub runtime: RuntimeManifest,
    pub presentation: PresentationManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameManifest {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub entry: String,
    pub logical_width: u32,
    pub logical_height: u32,
    pub asset_roots: Vec<String>,
    /// Exact logical asset paths intentionally kept out of the runtime pak.
    /// This is useful for source-only art and fonts retained for design work
    /// but not referenced by a shipping presentation.
    #[serde(default)]
    pub asset_excludes: Vec<String>,
    /// Ordered, project-bundled fonts used for every Player target.
    ///
    /// The list is intentionally logical asset paths rather than host family
    /// names. Native and Web load exactly these bytes and never discover a
    /// Windows/Linux/browser system font as part of the game contract.
    #[serde(default)]
    pub fonts: Vec<String>,
    /// Optional scheduling roles for shipped assets. Unlisted assets remain
    /// in the boot pack for backwards-compatible startup behavior.
    #[serde(default)]
    pub asset_pack_roles: BTreeMap<String, String>,
    pub save_namespace: String,
    /// Save namespaces that this release deliberately retires before opening
    /// the current namespace.  This remains opt-in: a project cannot erase a
    /// record merely by changing its current save name.
    #[serde(default)]
    pub legacy_save_namespaces: Vec<String>,
}

/// Project-local frontend source. The directory must contain the game UI's
/// package manifest; the CLI validates and bundles it rather than inventing a
/// generic visual fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationManifest {
    pub frontend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid aria.toml: {problems}", problems = .problems.join("; "))]
pub struct ProjectValidationError {
    pub problems: Vec<String>,
}

impl ProjectManifest {
    pub fn from_toml(source: &str) -> Result<Self, ProjectValidationError> {
        let manifest: Self = toml::from_str(source).map_err(|error| ProjectValidationError {
            problems: vec![error.to_string()],
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn to_toml(&self) -> Result<String, ProjectValidationError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|error| ProjectValidationError {
            problems: vec![error.to_string()],
        })
    }

    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        let mut problems = Vec::new();
        if self.schema != PROJECT_SCHEMA {
            problems.push(format!(
                "schema must be {PROJECT_SCHEMA}, got {}",
                self.schema
            ));
        }
        if !valid_game_id(&self.game.id) {
            problems.push(
                "game.id must use lowercase ASCII letters, digits, dots, hyphens, or underscores"
                    .to_owned(),
            );
        }
        if self.game.version.trim().is_empty() {
            problems.push("game.version must not be empty".to_owned());
        }
        if self.runtime.logical_width == 0 || self.runtime.logical_height == 0 {
            problems.push("logical resolution must be non-zero".to_owned());
        }
        if self.runtime.logical_width > 16_384 || self.runtime.logical_height > 16_384 {
            problems
                .push("logical resolution must not exceed 16384 in either dimension".to_owned());
        }
        validate_canonical_logical_path("runtime.entry", &self.runtime.entry, &mut problems);
        if !self.runtime.entry.to_ascii_lowercase().ends_with(".aria") {
            problems.push("runtime.entry must name an .aria script".to_owned());
        }
        if self.runtime.asset_roots.is_empty() {
            problems.push("runtime.asset_roots must contain at least one root".to_owned());
        }
        let mut asset_roots = BTreeMap::new();
        for root in &self.runtime.asset_roots {
            validate_canonical_logical_path("runtime.asset_roots", root, &mut problems);
            if let Ok(key) = crate::compiler::portable_path_key(root)
                && let Some(existing) = asset_roots.insert(key, root)
                && existing != root
            {
                problems.push(format!(
                    "runtime.asset_roots '{existing}' and '{root}' collide on a case-insensitive filesystem"
                ));
            }
        }
        let mut asset_excludes = BTreeMap::new();
        for excluded in &self.runtime.asset_excludes {
            validate_canonical_logical_path("runtime.asset_excludes", excluded, &mut problems);
            if !self
                .runtime
                .asset_roots
                .iter()
                .any(|root| is_under_asset_root(excluded, root))
            {
                problems.push(format!(
                    "runtime.asset_excludes '{excluded}' must be located below one of runtime.asset_roots"
                ));
            }
            if let Ok(key) = crate::compiler::portable_path_key(excluded)
                && let Some(existing) = asset_excludes.insert(key, excluded)
                && existing != excluded
            {
                problems.push(format!(
                    "runtime.asset_excludes '{existing}' and '{excluded}' collide on a case-insensitive filesystem"
                ));
            }
        }
        if self.runtime.fonts.len() > 32 {
            problems.push("runtime.fonts may contain at most 32 bundled fonts".to_owned());
        }
        let mut fonts = BTreeMap::new();
        for font in &self.runtime.fonts {
            validate_canonical_logical_path("runtime.fonts", font, &mut problems);
            if !is_font_path(font) {
                problems.push(format!(
                    "runtime.fonts '{font}' must use a .ttf, .otf, .ttc, or .otc font asset"
                ));
            }
            if !self
                .runtime
                .asset_roots
                .iter()
                .any(|root| is_under_asset_root(font, root))
            {
                problems.push(format!(
                    "runtime.fonts '{font}' must be located below one of runtime.asset_roots"
                ));
            }
            if self
                .runtime
                .asset_excludes
                .iter()
                .any(|excluded| excluded == font)
            {
                problems.push(format!(
                    "runtime.fonts '{font}' must not also appear in runtime.asset_excludes"
                ));
            }
            if let Ok(key) = crate::compiler::portable_path_key(font)
                && let Some(existing) = fonts.insert(key, font)
                && existing != font
            {
                problems.push(format!(
                    "runtime.fonts '{existing}' and '{font}' collide on a case-insensitive filesystem"
                ));
            }
        }
        let mut pack_roles = BTreeMap::new();
        for (asset, role) in &self.runtime.asset_pack_roles {
            validate_canonical_logical_path("runtime.asset_pack_roles", asset, &mut problems);
            if !self
                .runtime
                .asset_roots
                .iter()
                .any(|root| is_under_asset_root(asset, root))
            {
                problems.push(format!(
                    "runtime.asset_pack_roles asset '{asset}' must be located below one of runtime.asset_roots"
                ));
            }
            if self
                .runtime
                .asset_excludes
                .iter()
                .any(|excluded| excluded == asset)
            {
                problems.push(format!(
                    "runtime.asset_pack_roles asset '{asset}' must not also appear in runtime.asset_excludes"
                ));
            }
            if !matches!(role.as_str(), "boot" | "hot" | "cold" | "overlay") {
                problems.push(format!(
                    "runtime.asset_pack_roles '{asset}' has unsupported role '{role}'; use boot, hot, cold, or overlay"
                ));
            }
            if let Ok(key) = crate::compiler::portable_path_key(asset)
                && let Some(existing) = pack_roles.insert(key, asset)
                && existing != asset
            {
                problems.push(format!(
                    "runtime.asset_pack_roles assets '{existing}' and '{asset}' collide on a case-insensitive filesystem"
                ));
            }
        }
        if self.runtime.save_namespace.trim().is_empty() {
            problems.push("runtime.save_namespace must not be empty".to_owned());
        }
        if self.runtime.save_namespace.contains(['/', '\\']) {
            problems.push("runtime.save_namespace must be a single logical name".to_owned());
        }
        let mut legacy_save_namespaces = BTreeSet::new();
        for namespace in &self.runtime.legacy_save_namespaces {
            if namespace.trim().is_empty() || namespace.contains(['/', '\\']) {
                problems.push(
                    "runtime.legacy_save_namespaces must contain single logical names".to_owned(),
                );
                continue;
            }
            if namespace == &self.runtime.save_namespace {
                problems.push(
                    "runtime.legacy_save_namespaces must not include runtime.save_namespace"
                        .to_owned(),
                );
            }
            if !legacy_save_namespaces.insert(namespace.clone()) {
                problems.push(format!(
                    "runtime.legacy_save_namespaces contains duplicate namespace '{namespace}'"
                ));
            }
        }
        validate_canonical_logical_path(
            "presentation.frontend",
            &self.presentation.frontend,
            &mut problems,
        );
        if self.presentation.frontend == "." {
            problems.push("presentation.frontend must name a project subdirectory".to_owned());
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(ProjectValidationError { problems })
        }
    }
}

fn valid_game_id(id: &str) -> bool {
    !id.is_empty()
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn validate_canonical_logical_path(field: &str, value: &str, problems: &mut Vec<String>) {
    if value.trim().is_empty() {
        problems.push(format!("{field} must not be empty"));
        return;
    }
    if value.starts_with('/')
        || value.starts_with('\\')
        || value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
    {
        problems.push(format!("{field} must be relative"));
    }
    if value.split(['/', '\\']).any(|part| part == "..") {
        problems.push(format!("{field} must not escape the project root"));
    }
    match crate::compiler::normalize_logical_path(value) {
        Ok(normalized) if normalized != value => problems.push(format!(
            "{field} must use canonical NFC '/' logical paths; use '{normalized}' instead of '{value}'"
        )),
        Ok(_) => {}
        Err(error) => problems.push(format!("{field} {error}")),
    }
}

fn is_under_asset_root(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_font_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "ttf" | "otf" | "ttc" | "otc"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> ProjectManifest {
        ProjectManifest {
            schema: 4,
            game: GameManifest {
                id: "jp.example.umikaze".to_owned(),
                version: "3.0.0".to_owned(),
                title: "海風".to_owned(),
            },
            runtime: RuntimeManifest {
                entry: "scripts/main.aria".to_owned(),
                logical_width: 1280,
                logical_height: 720,
                asset_roots: vec!["assets".to_owned()],
                asset_excludes: Vec::new(),
                fonts: Vec::new(),
                asset_pack_roles: BTreeMap::new(),
                save_namespace: "umikaze-v3".to_owned(),
                legacy_save_namespaces: Vec::new(),
            },
            presentation: PresentationManifest {
                frontend: "ui".to_owned(),
            },
        }
    }

    #[test]
    fn manifest_round_trips() {
        let text = manifest().to_toml().unwrap();
        assert_eq!(ProjectManifest::from_toml(&text).unwrap(), manifest());
    }

    #[test]
    fn manifest_rejects_parent_paths() {
        let mut value = manifest();
        value.runtime.entry = "../main.aria".to_owned();
        assert!(value.validate().is_err());
    }

    #[test]
    fn manifest_rejects_nonportable_or_outside_font_paths() {
        let mut value = manifest();
        value.runtime.fonts = vec!["fonts\\MIO.otf".to_owned()];
        let error = value.validate().unwrap_err();
        assert!(
            error
                .problems
                .iter()
                .any(|problem| problem.contains("canonical NFC '/'"))
        );
        assert!(
            error
                .problems
                .iter()
                .any(|problem| problem.contains("below one of runtime.asset_roots"))
        );
    }
}
