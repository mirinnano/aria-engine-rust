use std::collections::BTreeMap;

use aria_core::{SaveEnvelopeError, SaveEnvelopeV3};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SaveKey {
    namespace: String,
    slot: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Generation {
    number: u64,
    bytes: Vec<u8>,
}

/// In-memory reference model for the IndexedDB adapter. The PWA shell performs
/// the same insert-and-prune operation in one readwrite transaction.
#[derive(Debug, Clone)]
pub struct GenerationStore {
    keep: usize,
    generations: BTreeMap<SaveKey, Vec<Generation>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecoveredGeneration {
    pub envelope: SaveEnvelopeV3,
    pub generation: u64,
    pub skipped_corrupt_generations: usize,
}

impl GenerationStore {
    pub fn new(keep: usize) -> Result<Self, GenerationStoreError> {
        if keep < 2 {
            return Err(GenerationStoreError::TooFewGenerations);
        }
        Ok(Self {
            keep,
            generations: BTreeMap::new(),
        })
    }

    pub fn put(
        &mut self,
        namespace: &str,
        slot: u32,
        envelope: &SaveEnvelopeV3,
    ) -> Result<u64, GenerationStoreError> {
        validate_namespace(namespace)?;
        envelope.validate()?;
        let key = SaveKey {
            namespace: namespace.to_owned(),
            slot,
        };
        let generations = self.generations.entry(key).or_default();
        let number = generations
            .last()
            .map_or(1, |generation| generation.number.saturating_add(1));
        generations.push(Generation {
            number,
            bytes: envelope.encode()?,
        });
        if generations.len() > self.keep {
            generations.drain(..generations.len() - self.keep);
        }
        Ok(number)
    }

    pub fn recover(
        &self,
        namespace: &str,
        slot: u32,
    ) -> Result<Option<RecoveredGeneration>, GenerationStoreError> {
        validate_namespace(namespace)?;
        let key = SaveKey {
            namespace: namespace.to_owned(),
            slot,
        };
        let Some(generations) = self.generations.get(&key) else {
            return Ok(None);
        };
        let mut corrupt = 0;
        for generation in generations.iter().rev() {
            match SaveEnvelopeV3::decode(&generation.bytes) {
                Ok(envelope) => {
                    return Ok(Some(RecoveredGeneration {
                        envelope,
                        generation: generation.number,
                        skipped_corrupt_generations: corrupt,
                    }));
                }
                Err(_) => corrupt += 1,
            }
        }
        Err(GenerationStoreError::AllGenerationsCorrupt(corrupt))
    }

    #[cfg(test)]
    fn corrupt_latest(&mut self, namespace: &str, slot: u32) {
        let key = SaveKey {
            namespace: namespace.to_owned(),
            slot,
        };
        if let Some(generation) = self
            .generations
            .get_mut(&key)
            .and_then(|generations| generations.last_mut())
        {
            generation.bytes = b"interrupted indexeddb payload".to_vec();
        }
    }
}

fn validate_namespace(namespace: &str) -> Result<(), GenerationStoreError> {
    if namespace.trim().is_empty() || namespace.contains(['/', '\\']) {
        Err(GenerationStoreError::InvalidNamespace(namespace.to_owned()))
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum GenerationStoreError {
    #[error("at least two save generations are required")]
    TooFewGenerations,
    #[error("invalid save namespace '{0}'")]
    InvalidNamespace(String),
    #[error("save envelope error: {0}")]
    Envelope(#[from] SaveEnvelopeError),
    #[error("all {0} IndexedDB save generations are corrupt")]
    AllGenerationsCorrupt(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(value: u32) -> SaveEnvelopeV3 {
        SaveEnvelopeV3::new("jp.example.game", "3.0.0", u64::from(value), &value).unwrap()
    }

    #[test]
    fn latest_corrupt_generation_recovers_the_previous_transaction() {
        let mut store = GenerationStore::new(3).unwrap();
        store.put("game", 1, &save(1)).unwrap();
        store.put("game", 1, &save(2)).unwrap();
        store.corrupt_latest("game", 1);
        let recovered = store.recover("game", 1).unwrap().unwrap();
        assert_eq!(recovered.envelope.payload_as::<u32>().unwrap(), 1);
        assert_eq!(recovered.skipped_corrupt_generations, 1);
    }

    #[test]
    fn transaction_model_prunes_only_after_new_generation_exists() {
        let mut store = GenerationStore::new(2).unwrap();
        for value in 1..=3 {
            store.put("game", 1, &save(value)).unwrap();
        }
        let recovered = store.recover("game", 1).unwrap().unwrap();
        assert_eq!(recovered.generation, 3);
        assert_eq!(recovered.envelope.payload_as::<u32>().unwrap(), 3);
    }
}
