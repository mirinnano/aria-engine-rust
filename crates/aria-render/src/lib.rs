#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

//! Shared rendering adapter implementation. Platform integrations are added
//! behind crate features; deterministic batching remains available everywhere.

pub mod batch;
#[cfg(feature = "gpu")]
pub mod gpu_renderer;
#[cfg(feature = "gpu")]
pub mod gpu_text;
pub mod layout;
pub mod resources;
#[cfg(feature = "text-layout")]
pub mod text;

pub use batch::{BatchKey, PrimitiveKind, RenderBatch, build_batches};
#[cfg(feature = "gpu")]
pub use gpu_renderer::{
    ImageResolver, RasterImage, RenderSubmission, RenderSurfaceSize, WgpuRenderer,
    WgpuRendererError,
};
#[cfg(feature = "gpu")]
pub use gpu_text::{BundledFont, BundledFontError};
pub use layout::{SafeAreaInsets, UiLayoutEngine, ViewportTransform};
pub use resources::{GpuResourceRegistry, GpuResourceState};
