use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuResourceState {
    Ready,
    DeviceLost,
    Recovering,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuResourceRegistry {
    state: GpuResourceState,
    generation: u64,
    resident_assets: BTreeSet<String>,
    reload_queue: BTreeSet<String>,
}

impl Default for GpuResourceRegistry {
    fn default() -> Self {
        Self {
            state: GpuResourceState::Ready,
            generation: 0,
            resident_assets: BTreeSet::new(),
            reload_queue: BTreeSet::new(),
        }
    }
}

impl GpuResourceRegistry {
    #[must_use]
    pub fn state(&self) -> GpuResourceState {
        self.state
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn mark_resident(&mut self, logical_path: impl Into<String>) -> Result<(), ResourceError> {
        if self.state != GpuResourceState::Ready {
            return Err(ResourceError::NotReady(self.state));
        }
        self.resident_assets.insert(logical_path.into());
        Ok(())
    }

    pub fn device_lost(&mut self) {
        self.reload_queue = self.resident_assets.clone();
        self.resident_assets.clear();
        self.state = GpuResourceState::DeviceLost;
    }

    pub fn begin_recovery(&mut self) -> Result<Vec<String>, ResourceError> {
        if self.state != GpuResourceState::DeviceLost {
            return Err(ResourceError::InvalidTransition {
                from: self.state,
                to: GpuResourceState::Recovering,
            });
        }
        self.state = GpuResourceState::Recovering;
        Ok(self.reload_queue.iter().cloned().collect())
    }

    pub fn recovered_asset(&mut self, logical_path: &str) -> Result<(), ResourceError> {
        if self.state != GpuResourceState::Recovering {
            return Err(ResourceError::NotRecovering);
        }
        if self.reload_queue.remove(logical_path) {
            self.resident_assets.insert(logical_path.to_owned());
        }
        Ok(())
    }

    pub fn finish_recovery(&mut self) -> Result<(), ResourceError> {
        if self.state != GpuResourceState::Recovering {
            return Err(ResourceError::NotRecovering);
        }
        if !self.reload_queue.is_empty() {
            return Err(ResourceError::AssetsPending(
                self.reload_queue.iter().cloned().collect(),
            ));
        }
        self.generation = self.generation.saturating_add(1);
        self.state = GpuResourceState::Ready;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("GPU resources are not ready: {0:?}")]
    NotReady(GpuResourceState),
    #[error("invalid GPU state transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: GpuResourceState,
        to: GpuResourceState,
    },
    #[error("GPU resource registry is not recovering")]
    NotRecovering,
    #[error("assets still need reload: {0:?}")]
    AssetsPending(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_loss_requires_every_resident_asset_to_reload() {
        let mut registry = GpuResourceRegistry::default();
        registry.mark_resident("bg/sea.webp").unwrap();
        registry.mark_resident("ch/mio.webp").unwrap();
        registry.device_lost();
        let pending = registry.begin_recovery().unwrap();
        assert_eq!(pending, ["bg/sea.webp", "ch/mio.webp"]);
        registry.recovered_asset("bg/sea.webp").unwrap();
        assert!(matches!(
            registry.finish_recovery(),
            Err(ResourceError::AssetsPending(_))
        ));
        registry.recovered_asset("ch/mio.webp").unwrap();
        registry.finish_recovery().unwrap();
        assert_eq!(registry.state(), GpuResourceState::Ready);
        assert_eq!(registry.generation(), 1);
    }
}
