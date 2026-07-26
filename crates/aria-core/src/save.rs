use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SAVE_SCHEMA_V3: u32 = 3;

/// Platform-neutral V3 save envelope. The timestamp is supplied by the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveEnvelopeV3 {
    pub schema_version: u32,
    pub game_id: String,
    pub engine_version: String,
    pub timestamp_unix_ms: u64,
    pub payload: serde_json::Value,
    pub checksum: String,
}

#[derive(Serialize)]
struct ChecksumMaterial<'a> {
    schema_version: u32,
    game_id: &'a str,
    engine_version: &'a str,
    timestamp_unix_ms: u64,
    payload: &'a serde_json::Value,
}

impl SaveEnvelopeV3 {
    pub fn new<T: Serialize>(
        game_id: impl Into<String>,
        engine_version: impl Into<String>,
        timestamp_unix_ms: u64,
        payload: &T,
    ) -> Result<Self, SaveEnvelopeError> {
        let payload = serde_json::to_value(payload).map_err(SaveEnvelopeError::Serialize)?;
        let mut envelope = Self {
            schema_version: SAVE_SCHEMA_V3,
            game_id: game_id.into(),
            engine_version: engine_version.into(),
            timestamp_unix_ms,
            payload,
            checksum: String::new(),
        };
        envelope.checksum = envelope.calculate_checksum()?;
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), SaveEnvelopeError> {
        if self.schema_version != SAVE_SCHEMA_V3 {
            return Err(SaveEnvelopeError::UnsupportedSchema(self.schema_version));
        }
        if self.game_id.trim().is_empty() {
            return Err(SaveEnvelopeError::MissingGameId);
        }
        let actual = self.calculate_checksum()?;
        if !constant_time_ascii_equal(actual.as_bytes(), self.checksum.as_bytes()) {
            return Err(SaveEnvelopeError::ChecksumMismatch);
        }
        Ok(())
    }

    pub fn validate_for_game(&self, game_id: &str) -> Result<(), SaveEnvelopeError> {
        self.validate()?;
        if self.game_id != game_id {
            return Err(SaveEnvelopeError::WrongGame {
                expected: game_id.to_owned(),
                actual: self.game_id.clone(),
            });
        }
        Ok(())
    }

    pub fn payload_as<T: for<'de> Deserialize<'de>>(&self) -> Result<T, SaveEnvelopeError> {
        self.validate()?;
        serde_json::from_value(self.payload.clone()).map_err(SaveEnvelopeError::Deserialize)
    }

    pub fn encode(&self) -> Result<Vec<u8>, SaveEnvelopeError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(SaveEnvelopeError::Serialize)
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, SaveEnvelopeError> {
        let envelope: Self =
            serde_json::from_slice(encoded).map_err(SaveEnvelopeError::Deserialize)?;
        envelope.validate()?;
        Ok(envelope)
    }

    fn calculate_checksum(&self) -> Result<String, SaveEnvelopeError> {
        let material = ChecksumMaterial {
            schema_version: self.schema_version,
            game_id: &self.game_id,
            engine_version: &self.engine_version,
            timestamp_unix_ms: self.timestamp_unix_ms,
            payload: &self.payload,
        };
        // A save crosses storage and Web string boundaries before it is
        // verified again. `serde_json::Value` can retain an f32's original
        // decimal rendering while a decoded JSON number is represented as an
        // f64 (and therefore has a different shortest rendering). Reparse
        // once into the transport representation, then hash a canonical
        // value rather than incidental number/map layouts.
        let value = serde_json::to_value(material).map_err(SaveEnvelopeError::Serialize)?;
        let transport = serde_json::to_vec(&value).map_err(SaveEnvelopeError::Serialize)?;
        let value = serde_json::from_slice(&transport).map_err(SaveEnvelopeError::Deserialize)?;
        let bytes =
            serde_json::to_vec(&canonical_json(value)).map_err(SaveEnvelopeError::Serialize)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

fn canonical_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json).collect())
        }
        serde_json::Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_json(value)))
                    .collect(),
            )
        }
        value => value,
    }
}

fn constant_time_ascii_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[derive(Debug, Error)]
pub enum SaveEnvelopeError {
    #[error("unsupported save schema {0}")]
    UnsupportedSchema(u32),
    #[error("save game ID is empty")]
    MissingGameId,
    #[error("save checksum mismatch")]
    ChecksumMismatch,
    #[error("save belongs to '{actual}', expected '{expected}'")]
    WrongGame { expected: String, actual: String },
    #[error("failed to serialize save envelope: {0}")]
    Serialize(serde_json::Error),
    #[error("failed to deserialize save envelope: {0}")]
    Deserialize(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{CompiledProgram, LogicalSize, Vm, VmSnapshot};

    use super::*;

    #[test]
    fn save_envelope_round_trips_and_is_game_scoped() {
        let payload = BTreeMap::from([("route".to_owned(), 2), ("chapter".to_owned(), 4)]);
        let save = SaveEnvelopeV3::new("jp.example.game", "3.0.0", 42, &payload).unwrap();
        let encoded = save.encode().unwrap();
        let decoded = SaveEnvelopeV3::decode(&encoded).unwrap();
        decoded.validate_for_game("jp.example.game").unwrap();
        assert_eq!(
            decoded.payload_as::<BTreeMap<String, i32>>().unwrap(),
            payload
        );
        assert!(matches!(
            decoded.validate_for_game("another.game"),
            Err(SaveEnvelopeError::WrongGame { .. })
        ));
    }

    #[test]
    fn edited_payload_is_rejected() {
        let mut save = SaveEnvelopeV3::new("jp.example.game", "3.0.0", 42, &1).unwrap();
        save.payload = serde_json::json!(2);
        assert!(matches!(
            save.validate(),
            Err(SaveEnvelopeError::ChecksumMismatch)
        ));
    }

    #[test]
    fn vm_snapshot_survives_json_string_transport() {
        let vm = Vm::new(
            CompiledProgram::empty("jp.example.game"),
            LogicalSize {
                width: 1_280,
                height: 720,
            },
        )
        .unwrap();
        let envelope = SaveEnvelopeV3::new("jp.example.game", "3.0.0", 42, &vm.snapshot()).unwrap();
        let transport = serde_json::to_string(&envelope).unwrap();
        let decoded = SaveEnvelopeV3::decode(transport.as_bytes()).unwrap();
        let restored: VmSnapshot = decoded.payload_as().unwrap();
        assert_eq!(restored.schema_version, vm.snapshot().schema_version);
        assert_eq!(restored.game_id, vm.snapshot().game_id);
        assert_eq!(restored.ui.route, vm.snapshot().ui.route);
        assert!(restored.ui.scroll_offsets.is_empty());
        assert_eq!(restored.ui.route, "dialogue");
    }
}
