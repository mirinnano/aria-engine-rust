//! Chooses which of the discovered plugin packages a game runs.
//!
//! Hosts collect every `.ariaplugin` they find (the install folder, then the
//! user-data folder) and pass the results here with the game's requirements.
//! For each plugin id the newest verified build that satisfies the game's
//! range wins, so dropping a newer file into user data updates a plugin
//! without touching the engine or the PAK.

use std::collections::BTreeMap;

use semver::{Version, VersionReq};

use crate::package::{PackageError, PluginPackage};

/// One file a host found, with where it came from for diagnostics.
#[derive(Debug)]
pub struct Candidate {
    pub source: String,
    pub package: Result<PluginPackage, PackageError>,
}

#[derive(Debug, Default)]
pub struct Selection {
    /// Chosen packages, ordered by plugin id.
    pub plugins: Vec<PluginPackage>,
    /// Where each chosen package came from, parallel to `plugins`.
    pub sources: Vec<String>,
    /// Files that were skipped, and why.
    pub skipped: Vec<String>,
    /// Required plugins with no acceptable build.
    pub missing: Vec<String>,
}

impl Selection {
    /// Whether the game can start: every required plugin is present.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }
}

#[must_use]
pub fn select(candidates: Vec<Candidate>, require: &BTreeMap<String, VersionReq>) -> Selection {
    let mut selection = Selection::default();
    let mut best: BTreeMap<String, (Version, PluginPackage, String)> = BTreeMap::new();
    for candidate in candidates {
        let package = match candidate.package {
            Ok(package) => package,
            Err(error) => {
                selection
                    .skipped
                    .push(format!("{}: {error}", candidate.source));
                continue;
            }
        };
        let manifest = &package.registration.manifest;
        let Ok(version) = manifest.semver() else {
            selection
                .skipped
                .push(format!("{}: version is not semver", candidate.source));
            continue;
        };
        if let Some(range) = require.get(&manifest.id)
            && !range.matches(&version)
        {
            selection.skipped.push(format!(
                "{}: {} {} is outside the required range {range}",
                candidate.source, manifest.id, manifest.version
            ));
            continue;
        }
        match best.get(&manifest.id) {
            Some((current, _, _)) if *current >= version => selection.skipped.push(format!(
                "{}: {} {} is not newer than {current}",
                candidate.source, manifest.id, manifest.version
            )),
            _ => {
                let id = manifest.id.clone();
                best.insert(id, (version, package, candidate.source));
            }
        }
    }
    for (id, range) in require {
        if !best.contains_key(id) {
            selection.missing.push(format!("{id} {range}"));
        }
    }
    for (_, package, source) in best.into_values() {
        selection.plugins.push(package);
        selection.sources.push(source);
    }
    selection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{open_package, pack};
    use crate::tests::guest_with_manifest;
    use ed25519_dalek::SigningKey;

    fn candidate(source: &str, id: &str, version: &str) -> Candidate {
        let wasm = guest_with_manifest(&format!(
            r#"{{"api_version":2,"id":"{id}","version":"{version}","capabilities":["state"]}}"#
        ));
        let key = [3; 32];
        let bytes = pack(&wasm, &BTreeMap::new(), "publisher", &key).unwrap();
        let trusted = [(
            "publisher".to_owned(),
            SigningKey::from_bytes(&key).verifying_key().to_bytes(),
        )]
        .into();
        Candidate {
            source: source.into(),
            package: open_package(&bytes, &trusted),
        }
    }

    fn version_of(selection: &Selection, id: &str) -> Option<String> {
        selection
            .plugins
            .iter()
            .find(|package| package.registration.manifest.id == id)
            .map(|package| package.registration.manifest.version.clone())
    }

    #[test]
    fn the_newest_build_inside_the_range_wins() {
        let require = BTreeMap::from([("story".to_owned(), VersionReq::parse("^1.2").unwrap())]);
        let selection = select(
            vec![
                candidate("install/story.ariaplugin", "story", "1.2.0"),
                candidate("user/story-1.4.ariaplugin", "story", "1.4.1"),
                candidate("user/story-2.ariaplugin", "story", "2.0.0"),
                candidate("user/extra.ariaplugin", "extra", "0.1.0"),
            ],
            &require,
        );
        assert!(selection.is_complete());
        assert_eq!(version_of(&selection, "story").as_deref(), Some("1.4.1"));
        assert_eq!(version_of(&selection, "extra").as_deref(), Some("0.1.0"));
        assert!(selection.skipped.iter().any(|line| line.contains("2.0.0")));
    }

    #[test]
    fn a_missing_requirement_and_a_broken_file_are_reported() {
        let require = BTreeMap::from([("story".to_owned(), VersionReq::parse("^1").unwrap())]);
        let selection = select(
            vec![Candidate {
                source: "user/broken.ariaplugin".into(),
                package: Err(PackageError::BadSignature),
            }],
            &require,
        );
        assert!(!selection.is_complete());
        assert_eq!(selection.missing, ["story ^1"]);
        assert!(selection.skipped[0].contains("broken.ariaplugin"));
    }
}
