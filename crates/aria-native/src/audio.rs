use std::collections::BTreeMap;

use aria_core::protocol::{AudioBus, AudioCommand};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
use std::io::Cursor;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
use std::path::{Component, Path, PathBuf};
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
use std::time::Duration;

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayingSound {
    pub bus: AudioBus,
    pub id: String,
    pub asset: String,
    pub looping: bool,
    pub volume: f32,
    pub fade_in_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioSemanticState {
    pub playing: BTreeMap<String, PlayingSound>,
    pub bus_volumes: BTreeMap<AudioBus, f32>,
}

impl Default for AudioSemanticState {
    fn default() -> Self {
        Self {
            playing: BTreeMap::new(),
            bus_volumes: BTreeMap::from([
                (AudioBus::Bgm, 1.0),
                (AudioBus::SoundEffect, 1.0),
                (AudioBus::Voice, 1.0),
            ]),
        }
    }
}

impl AudioSemanticState {
    pub fn apply(&mut self, command: &AudioCommand) -> Result<(), AudioAdapterError> {
        match command {
            AudioCommand::Play {
                bus,
                id,
                asset,
                looping,
                volume,
                fade_in_ms,
            } => {
                validate_volume(*volume)?;
                self.playing.insert(
                    key(*bus, id),
                    PlayingSound {
                        bus: *bus,
                        id: id.clone(),
                        asset: asset.clone(),
                        looping: *looping,
                        volume: *volume,
                        fade_in_ms: *fade_in_ms,
                    },
                );
            }
            AudioCommand::Stop { bus, id, .. } => {
                if let Some(id) = id {
                    self.playing.remove(&key(*bus, id));
                } else {
                    self.playing.retain(|_, sound| sound.bus != *bus);
                }
            }
            AudioCommand::SetBusVolume { bus, volume, .. } => {
                validate_volume(*volume)?;
                self.bus_volumes.insert(*bus, *volume);
            }
        }
        Ok(())
    }
}

fn validate_volume(volume: f32) -> Result<(), AudioAdapterError> {
    if volume.is_finite() && (0.0..=1.0).contains(&volume) {
        Ok(())
    } else {
        Err(AudioAdapterError::InvalidVolume(volume))
    }
}

fn key(bus: AudioBus, id: &str) -> String {
    format!("{bus:?}:{id}")
}

#[derive(Debug, Error)]
pub enum AudioAdapterError {
    #[error("audio volume must be finite and in 0..=1, got {0}")]
    InvalidVolume(f32),
}

/// Native Kira/cpal implementation of the platform-neutral audio contract.
///
/// Asset names remain logical paths in `aria-core`; only this adapter resolves
/// them to files. Pack-backed builds can materialize or cache the bytes before
/// invoking this adapter without changing VM semantics.
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub struct KiraAudioAdapter {
    asset_root: PathBuf,
    manager: AudioManager<DefaultBackend>,
    semantic: AudioSemanticState,
    handles: BTreeMap<String, NativeSound>,
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
struct NativeSound {
    bus: AudioBus,
    base_volume: f32,
    handle: StaticSoundHandle,
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
impl std::fmt::Debug for KiraAudioAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KiraAudioAdapter")
            .field("asset_root", &self.asset_root)
            .field("semantic", &self.semantic)
            .field("active_handle_count", &self.handles.len())
            .finish_non_exhaustive()
    }
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
impl std::fmt::Debug for NativeSound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeSound")
            .field("bus", &self.bus)
            .field("base_volume", &self.base_volume)
            .field("playback_state", &self.handle.state())
            .finish()
    }
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
impl KiraAudioAdapter {
    pub fn new(asset_root: impl Into<PathBuf>) -> Result<Self, KiraAudioError> {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|error| KiraAudioError::Start(error.to_string()))?;
        Ok(Self {
            asset_root: asset_root.into(),
            manager,
            semantic: AudioSemanticState::default(),
            handles: BTreeMap::new(),
        })
    }

    #[must_use]
    pub const fn semantic_state(&self) -> &AudioSemanticState {
        &self.semantic
    }

    /// Stops every device-local sound before a save restore rebuilds the
    /// shared semantic state. The restored VM then emits bus/track commands
    /// through the same adapter contract on its next frame.
    pub fn stop_all(&mut self) {
        for (_, mut sound) in std::mem::take(&mut self.handles) {
            sound.handle.stop(Tween::default());
        }
        self.semantic = AudioSemanticState::default();
    }

    pub fn apply(&mut self, command: &AudioCommand) -> Result<(), KiraAudioError> {
        let asset_bytes = match command {
            AudioCommand::Play { asset, .. } => {
                let path = resolve_logical_asset(&self.asset_root, asset)?;
                Some(std::fs::read(path).map_err(|error| KiraAudioError::Read {
                    asset: asset.clone(),
                    message: error.to_string(),
                })?)
            }
            AudioCommand::Stop { .. } | AudioCommand::SetBusVolume { .. } => None,
        };
        self.apply_bytes(command, asset_bytes)
    }

    /// Applies a command using bytes read from a pak or another non-filesystem
    /// source. This keeps the VM's logical asset contract identical for loose
    /// projects and packaged Players.
    pub fn apply_bytes(
        &mut self,
        command: &AudioCommand,
        asset_bytes: Option<Vec<u8>>,
    ) -> Result<(), KiraAudioError> {
        // Validate the entire shared contract before touching the device.
        let mut next_semantic = self.semantic.clone();
        next_semantic.apply(command)?;

        match command {
            AudioCommand::Play {
                bus,
                id,
                asset,
                looping,
                volume,
                fade_in_ms,
            } => {
                let bytes = asset_bytes.ok_or_else(|| KiraAudioError::MissingAssetBytes {
                    asset: asset.clone(),
                })?;
                let mut sound =
                    StaticSoundData::from_cursor(Cursor::new(bytes)).map_err(|error| {
                        KiraAudioError::Decode {
                            asset: asset.clone(),
                            message: error.to_string(),
                        }
                    })?;
                if *looping {
                    sound = sound.loop_region(..);
                }
                let bus_volume = self.semantic.bus_volumes.get(bus).copied().unwrap_or(1.0);
                sound = sound.volume(amplitude_to_decibels(*volume * bus_volume));
                if *fade_in_ms > 0 {
                    sound = sound.fade_in_tween(tween(*fade_in_ms));
                }
                let handle = self
                    .manager
                    .play(sound)
                    .map_err(|error| KiraAudioError::Play(error.to_string()))?;
                if let Some(mut old) = self.handles.insert(
                    key(*bus, id),
                    NativeSound {
                        bus: *bus,
                        base_volume: *volume,
                        handle,
                    },
                ) {
                    old.handle.stop(Tween::default());
                }
            }
            AudioCommand::Stop {
                bus,
                id,
                fade_out_ms,
            } => {
                let keys: Vec<_> = self
                    .handles
                    .iter()
                    .filter(|(sound_key, sound)| {
                        sound.bus == *bus
                            && id
                                .as_ref()
                                .is_none_or(|id| sound_key.as_str() == key(*bus, id))
                    })
                    .map(|(sound_key, _)| sound_key.clone())
                    .collect();
                for sound_key in keys {
                    if let Some(mut sound) = self.handles.remove(&sound_key) {
                        sound.handle.stop(tween(*fade_out_ms));
                    }
                }
            }
            AudioCommand::SetBusVolume {
                bus,
                volume,
                fade_ms,
            } => {
                for sound in self.handles.values_mut().filter(|sound| sound.bus == *bus) {
                    sound.handle.set_volume(
                        amplitude_to_decibels(sound.base_volume * *volume),
                        tween(*fade_ms),
                    );
                }
            }
        }
        self.semantic = next_semantic;
        Ok(())
    }
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
fn resolve_logical_asset(root: &Path, logical: &str) -> Result<PathBuf, KiraAudioError> {
    let logical_path = Path::new(logical);
    if logical_path.as_os_str().is_empty()
        || logical_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(KiraAudioError::InvalidAssetPath(logical.to_owned()));
    }
    Ok(root.join(logical_path))
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
fn amplitude_to_decibels(amplitude: f32) -> Decibels {
    if amplitude <= 0.0 {
        Decibels::SILENCE
    } else {
        Decibels((20.0 * amplitude.log10()).max(Decibels::SILENCE.0))
    }
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
fn tween(duration_ms: u32) -> Tween {
    Tween {
        duration: Duration::from_millis(u64::from(duration_ms)),
        ..Tween::default()
    }
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
#[derive(Debug, Error)]
pub enum KiraAudioError {
    #[error(transparent)]
    Contract(#[from] AudioAdapterError),
    #[error("failed to initialize Kira/cpal: {0}")]
    Start(String),
    #[error("invalid logical audio asset path: {0}")]
    InvalidAssetPath(String),
    #[error("cannot read audio asset {asset}: {message}")]
    Read { asset: String, message: String },
    #[error("audio command needs pak or filesystem bytes for {asset}")]
    MissingAssetBytes { asset: String },
    #[error("cannot decode audio asset {asset}: {message}")]
    Decode { asset: String, message: String },
    #[error("Kira could not play a decoded sound: {0}")]
    Play(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_without_id_stops_only_the_selected_bus() {
        let mut state = AudioSemanticState::default();
        for bus in [AudioBus::Bgm, AudioBus::Voice] {
            state
                .apply(&AudioCommand::Play {
                    bus,
                    id: "main".to_owned(),
                    asset: "sound.ogg".to_owned(),
                    looping: true,
                    volume: 1.0,
                    fade_in_ms: 100,
                })
                .unwrap();
        }
        state
            .apply(&AudioCommand::Stop {
                bus: AudioBus::Bgm,
                id: None,
                fade_out_ms: 250,
            })
            .unwrap();
        assert_eq!(state.playing.len(), 1);
        assert_eq!(state.playing.values().next().unwrap().bus, AudioBus::Voice);
    }

    #[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
    #[test]
    fn native_adapter_rejects_paths_that_escape_the_asset_root() {
        assert!(matches!(
            resolve_logical_asset(Path::new("assets"), "../private.ogg"),
            Err(KiraAudioError::InvalidAssetPath(_))
        ));
        assert_eq!(
            resolve_logical_asset(Path::new("assets"), "audio/bgm.ogg").unwrap(),
            Path::new("assets/audio/bgm.ogg")
        );
    }

    #[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
    #[test]
    fn linear_volume_is_converted_to_kira_decibels() {
        assert_eq!(amplitude_to_decibels(0.0), Decibels::SILENCE);
        assert!((amplitude_to_decibels(0.5).0 + 6.0206).abs() < 0.001);
        assert_eq!(amplitude_to_decibels(1.0), Decibels::IDENTITY);
    }
}
