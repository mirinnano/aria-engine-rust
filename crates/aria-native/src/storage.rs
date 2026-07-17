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
        if namespace.trim().is_empty()
            || namespace.contains(['/', '\\'])
            || namespace == "."
            || namespace == ".."
        {
            return Err(SaveStoreError::InvalidNamespace(namespace));
        }
        Ok(Self {
            root: root.into(),
            namespace,
        })
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
}
