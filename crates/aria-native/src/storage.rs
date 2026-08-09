use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use aria_core::{SaveEnvelopeError, SaveEnvelopeV3};
use atomic_write_file::AtomicWriteFile;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AtomicSaveStore {
    root: PathBuf,
    namespace: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSave {
    pub envelope: SaveEnvelopeV3,
    pub recovered_from_previous: bool,
}

impl AtomicSaveStore {
    pub fn new(
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<Self, SaveStoreError> {
        let namespace = namespace.into();
        validate_namespace(&namespace)?;
        Ok(Self {
            root: root.into(),
            namespace,
        })
    }

    /// Removes exactly one validated namespace below `root`.
    ///
    /// Releases use this only for names explicitly listed as legacy in their
    /// manifest; no caller can pass a path or escape the chosen save root.
    pub fn purge_namespace(
        root: impl AsRef<Path>,
        namespace: &str,
    ) -> Result<bool, SaveStoreError> {
        validate_namespace(namespace)?;
        let directory = root.as_ref().join(namespace);
        if !directory.exists() {
            return Ok(true);
        }
        fs::remove_dir_all(directory)?;
        Ok(true)
    }

    pub fn save(&self, slot: u32, envelope: &SaveEnvelopeV3) -> Result<(), SaveStoreError> {
        envelope.validate()?;
        let directory = self.directory();
        fs::create_dir_all(&directory)?;
        let current = self.current_path(slot);
        let previous = self.previous_path(slot);

        if let Ok(existing) = fs::read(&current)
            && SaveEnvelopeV3::decode(&existing).is_ok()
        {
            write_atomic(&previous, &existing)?;
        }
        let encoded = envelope.encode()?;
        write_atomic(&current, &encoded)
    }

    /// Promotes a recovered older generation to current without rotating the
    /// existing current file into `previous`. This is deliberately separate
    /// from [`Self::save`]: the current file may be checksum-valid yet belong
    /// to incompatible bytecode, while `previous` is the only compatible
    /// recovery point and must survive an interrupted promotion.
    pub fn promote_recovered(
        &self,
        slot: u32,
        envelope: &SaveEnvelopeV3,
    ) -> Result<(), SaveStoreError> {
        envelope.validate()?;
        let directory = self.directory();
        fs::create_dir_all(&directory)?;
        let encoded = envelope.encode()?;
        write_atomic(&self.current_path(slot), &encoded)
    }

    pub fn load(&self, slot: u32) -> Result<Option<LoadedSave>, SaveStoreError> {
        let current = self.current_path(slot);
        match fs::read(&current) {
            Ok(bytes) => match SaveEnvelopeV3::decode(&bytes) {
                Ok(envelope) => {
                    return Ok(Some(LoadedSave {
                        envelope,
                        recovered_from_previous: false,
                    }));
                }
                Err(current_error) => {
                    let previous = self.previous_path(slot);
                    match fs::read(&previous) {
                        Ok(bytes) => {
                            let envelope =
                                SaveEnvelopeV3::decode(&bytes).map_err(|previous_error| {
                                    SaveStoreError::BothGenerationsInvalid {
                                        current: current_error.to_string(),
                                        previous: previous_error.to_string(),
                                    }
                                })?;
                            return Ok(Some(LoadedSave {
                                envelope,
                                recovered_from_previous: true,
                            }));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            return Err(SaveStoreError::InvalidCurrent(current_error));
                        }
                        Err(error) => return Err(SaveStoreError::Io(error)),
                    }
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SaveStoreError::Io(error)),
        }
        let previous = self.previous_path(slot);
        match fs::read(previous) {
            Ok(bytes) => Ok(Some(LoadedSave {
                envelope: SaveEnvelopeV3::decode(&bytes)?,
                recovered_from_previous: true,
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(SaveStoreError::Io(error)),
        }
    }

    /// Returns every checksum-valid generation in newest-first order. Callers
    /// must still validate game/schema/program compatibility before replacing
    /// a live VM, because an envelope can be structurally sound yet belong to
    /// retired content.
    pub fn load_candidates(&self, slot: u32) -> Result<Vec<LoadedSave>, SaveStoreError> {
        let mut candidates = Vec::with_capacity(2);
        let mut found = false;
        let mut first_error = None;
        for (path, recovered_from_previous) in [
            (self.current_path(slot), false),
            (self.previous_path(slot), true),
        ] {
            match fs::read(path) {
                Ok(bytes) => {
                    found = true;
                    match SaveEnvelopeV3::decode(&bytes) {
                        Ok(envelope) => candidates.push(LoadedSave {
                            envelope,
                            recovered_from_previous,
                        }),
                        Err(error) if first_error.is_none() => first_error = Some(error),
                        Err(_) => {}
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(SaveStoreError::Io(error)),
            }
        }
        if candidates.is_empty() && found {
            return Err(SaveStoreError::InvalidCurrent(
                first_error.expect("a present invalid generation has a decode error"),
            ));
        }
        Ok(candidates)
    }

    #[must_use]
    pub fn directory(&self) -> PathBuf {
        self.root.join(&self.namespace)
    }

    fn current_path(&self, slot: u32) -> PathBuf {
        self.directory().join(format!("slot_{slot:04}.ariasave"))
    }

    fn previous_path(&self, slot: u32) -> PathBuf {
        self.directory()
            .join(format!("slot_{slot:04}.previous.ariasave"))
    }
}

fn validate_namespace(namespace: &str) -> Result<(), SaveStoreError> {
    if namespace.trim().is_empty()
        || namespace.contains(['/', '\\'])
        || namespace == "."
        || namespace == ".."
    {
        return Err(SaveStoreError::InvalidNamespace(namespace.to_owned()));
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SaveStoreError> {
    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.commit()?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum SaveStoreError {
    #[error("invalid save namespace '{0}'")]
    InvalidNamespace(String),
    #[error("save I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid save envelope: {0}")]
    Envelope(#[from] SaveEnvelopeError),
    #[error("current save is invalid and has no previous generation: {0}")]
    InvalidCurrent(SaveEnvelopeError),
    #[error("both save generations are invalid (current: {current}; previous: {previous})")]
    BothGenerationsInvalid { current: String, previous: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(value: i32, timestamp: u64) -> SaveEnvelopeV3 {
        SaveEnvelopeV3::new("jp.example.game", "3.0.0", timestamp, &value).unwrap()
    }

    #[test]
    fn interrupted_candidate_never_replaces_current_save() {
        let temp = tempfile::tempdir().unwrap();
        let store = AtomicSaveStore::new(temp.path(), "game").unwrap();
        store.save(1, &save(1, 1)).unwrap();

        let path = store.current_path(1);
        let mut candidate = AtomicWriteFile::open(&path).unwrap();
        candidate.write_all(b"partial").unwrap();
        drop(candidate);

        let loaded = store.load(1).unwrap().unwrap();
        assert_eq!(loaded.envelope.payload_as::<i32>().unwrap(), 1);
        assert!(!loaded.recovered_from_previous);
    }

    #[test]
    fn corrupt_current_recovers_previous_valid_generation() {
        let temp = tempfile::tempdir().unwrap();
        let store = AtomicSaveStore::new(temp.path(), "game").unwrap();
        store.save(1, &save(1, 1)).unwrap();
        store.save(1, &save(2, 2)).unwrap();
        fs::write(store.current_path(1), b"corrupt").unwrap();

        let loaded = store.load(1).unwrap().unwrap();
        assert_eq!(loaded.envelope.payload_as::<i32>().unwrap(), 1);
        assert!(loaded.recovered_from_previous);
    }

    #[test]
    fn candidates_keep_checksum_valid_current_and_previous_generations() {
        let temp = tempfile::tempdir().unwrap();
        let store = AtomicSaveStore::new(temp.path(), "game").unwrap();
        store.save(1, &save(1, 1)).unwrap();
        store.save(1, &save(2, 2)).unwrap();
        let candidates = store.load_candidates(1).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].envelope.payload_as::<i32>().unwrap(), 2);
        assert!(!candidates[0].recovered_from_previous);
        assert_eq!(candidates[1].envelope.payload_as::<i32>().unwrap(), 1);
        assert!(candidates[1].recovered_from_previous);
    }

    #[test]
    fn recovered_promotion_replaces_current_without_rotating_previous() {
        let temp = tempfile::tempdir().unwrap();
        let store = AtomicSaveStore::new(temp.path(), "game").unwrap();
        let compatible = save(1, 1);
        let incompatible = save(2, 2);
        store.save(1, &compatible).unwrap();
        store.save(1, &incompatible).unwrap();
        store.promote_recovered(1, &compatible).unwrap();

        let current = SaveEnvelopeV3::decode(&fs::read(store.current_path(1)).unwrap()).unwrap();
        let previous = SaveEnvelopeV3::decode(&fs::read(store.previous_path(1)).unwrap()).unwrap();
        assert_eq!(current.payload_as::<i32>().unwrap(), 1);
        assert_eq!(previous.payload_as::<i32>().unwrap(), 1);
    }

    #[test]
    fn purge_namespace_removes_only_the_explicit_legacy_directory() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = AtomicSaveStore::new(temp.path(), "umikaze-v3").unwrap();
        let current = AtomicSaveStore::new(temp.path(), "umikaze-v4").unwrap();
        legacy.save(1, &save(1, 1)).unwrap();
        current.save(1, &save(2, 2)).unwrap();

        assert!(AtomicSaveStore::purge_namespace(temp.path(), "umikaze-v3").unwrap());
        assert!(!legacy.directory().exists());
        assert!(current.directory().exists());
        assert_eq!(
            current
                .load(1)
                .unwrap()
                .unwrap()
                .envelope
                .payload_as::<i32>()
                .unwrap(),
            2
        );
    }

    #[test]
    fn purge_namespace_rejects_paths() {
        let temp = tempfile::tempdir().unwrap();
        assert!(matches!(
            AtomicSaveStore::purge_namespace(temp.path(), "../other"),
            Err(SaveStoreError::InvalidNamespace(_))
        ));
    }
}
