use aria_render::{GpuResourceRegistry, GpuResourceState};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsBackend {
    WebGpu,
    WebGl2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphicsCapabilities {
    pub webgpu: bool,
    pub webgl2: bool,
}

impl GraphicsCapabilities {
    pub fn preferred(self) -> Result<GraphicsBackend, GraphicsError> {
        if self.webgpu {
            Ok(GraphicsBackend::WebGpu)
        } else if self.webgl2 {
            Ok(GraphicsBackend::WebGl2)
        } else {
            Err(GraphicsError::NoSupportedBackend)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphicsContextState {
    pub backend: GraphicsBackend,
    pub resources: GpuResourceRegistry,
    pub suspended: bool,
}

impl GraphicsContextState {
    pub fn new(capabilities: GraphicsCapabilities) -> Result<Self, GraphicsError> {
        Ok(Self {
            backend: capabilities.preferred()?,
            resources: GpuResourceRegistry::default(),
            suspended: false,
        })
    }

    pub fn context_lost(&mut self) {
        self.resources.device_lost();
    }

    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    pub fn resume(&mut self) {
        self.suspended = false;
    }

    #[must_use]
    pub fn needs_recovery(&self) -> bool {
        self.resources.state() == GpuResourceState::DeviceLost
    }
}

#[derive(Debug, Error)]
pub enum GraphicsError {
    #[error("this browser supports neither WebGPU nor WebGL2")]
    NoSupportedBackend,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webgpu_is_preferred_and_context_loss_is_explicit() {
        let mut state = GraphicsContextState::new(GraphicsCapabilities {
            webgpu: true,
            webgl2: true,
        })
        .unwrap();
        assert_eq!(state.backend, GraphicsBackend::WebGpu);
        state.resources.mark_resident("bg/sea.webp").unwrap();
        state.context_lost();
        assert!(state.needs_recovery());
    }

    #[test]
    fn webgl2_is_the_fallback_not_a_separate_runtime() {
        let state = GraphicsContextState::new(GraphicsCapabilities {
            webgpu: false,
            webgl2: true,
        })
        .unwrap();
        assert_eq!(state.backend, GraphicsBackend::WebGl2);
    }
}
