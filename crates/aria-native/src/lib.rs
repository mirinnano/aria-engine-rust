#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

//! Native adapter for the Aria V3 core.

pub mod accessibility;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub mod assets;
pub mod audio;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub mod controller;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub mod desktop;
pub mod input;
/// Native-facing License Provider contract.
pub mod license {
    pub use aria_protection::{
        Entitlement, EntitlementRequest, LeaseRequest, LeaseStatus, LicenseAuthorization,
        LicenseError, LicenseLease, LicensePolicy, LicenseProvider,
    };

    /// The Player ABI intentionally exposes only entitlement and lease
    /// operations; VM, renderer, and package internals stay private to the
    /// adapter.
    pub use aria_protection::LicenseProvider as PlayerLicenseProvider;
}
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub mod player;
pub mod replay;
pub mod storage;

pub use input::{InputNormalizer, RawControl, RawInputEvent};
pub use replay::{ReplayResult, ReplayRunner, ReplayTape};
pub use storage::{AtomicSaveStore, LoadedSave};

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub use assets::{AssetProvider, NativeAssetStore};
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub use audio::KiraAudioAdapter;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub use controller::GilrsController;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub use desktop::WinitInputAdapter;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub use player::{NativePlayerConfig, NativePlayerError, default_save_root, run_desktop};
