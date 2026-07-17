#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

//! Web adapter for the Aria V3 core.

pub mod graphics;
pub mod runtime;
pub mod storage;

/// Web-facing License Provider contract. Browser key delivery is intentionally
/// outside this contract and may provide only short-lived keys at runtime.
pub mod license {
    pub use aria_protection::{
        Entitlement, EntitlementRequest, LeaseRequest, LeaseStatus, LicenseAuthorization,
        LicenseError, LicenseLease, LicensePolicy, LicenseProvider,
    };

    pub use aria_protection::LicenseProvider as PlayerLicenseProvider;
}

pub use graphics::{GraphicsBackend, GraphicsCapabilities, GraphicsContextState};
pub use runtime::PortableWebRuntime;
pub use storage::{GenerationStore, RecoveredGeneration};

#[cfg(target_arch = "wasm32")]
mod wasm_api {
    use aria_core::pak::PakArchive;
    use aria_core::protocol::LogicalSize;
    use aria_core::{CompiledProgram, InputSnapshot, VmSnapshot};
    use aria_protection::{PakPackage, StaticPakKeyProvider};
    use wasm_bindgen::prelude::*;

    use crate::PortableWebRuntime;

    /// Read-only integrity-checked Web pak. Logical paths are resolved inside
    /// Rust so the PWA never needs to know hashed archive entry names.
    #[wasm_bindgen]
    #[derive(Debug)]
    pub struct WebPak {
        inner: WebPakInner,
    }

    #[derive(Debug)]
    enum WebPakInner {
        Core(PakArchive),
        Protected(PakPackage),
    }

    #[wasm_bindgen]
    impl WebPak {
        #[wasm_bindgen(constructor)]
        pub fn new(bytes: &[u8]) -> Result<Self, JsValue> {
            let inner =
                PakArchive::open(bytes).map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(Self {
                inner: WebPakInner::Core(inner),
            })
        }

        /// Opens a signed/protected pack with keys delivered by the Web
        /// bootstrap. Keys are short-lived runtime inputs; they are never
        /// stored in the Core VM or in the package manifest.
        pub fn new_with_keys(
            bytes: &[u8],
            verification_key_id: &str,
            verification_key_hex: &str,
            encryption_key_id: &str,
            encryption_key_hex: &str,
        ) -> Result<Self, JsValue> {
            let verification = fixed_key(verification_key_hex)?;
            let mut provider = StaticPakKeyProvider::new()
                .with_verification_key(verification_key_id, verification);
            if !encryption_key_hex.is_empty() {
                let encryption = fixed_key(encryption_key_hex)?;
                provider = provider.with_encryption_key(
                    &aria_protection::PakEncryptionKey::from_bytes(encryption_key_id, encryption)
                        .map_err(|error| JsValue::from_str(&error.to_string()))?,
                );
            }
            let package = PakPackage::open(bytes, Some(&provider))
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(Self {
                inner: WebPakInner::Protected(package),
            })
        }

        pub fn read(&self, logical_path: &str) -> Result<Vec<u8>, JsValue> {
            match &self.inner {
                WebPakInner::Core(inner) => inner
                    .read(logical_path)
                    .map_err(|error| JsValue::from_str(&error.to_string())),
                WebPakInner::Protected(inner) => inner
                    .read(logical_path)
                    .map_err(|error| JsValue::from_str(&error.to_string())),
            }
        }

        pub fn game_id(&self) -> String {
            match &self.inner {
                WebPakInner::Core(inner) => inner.game_id().to_owned(),
                WebPakInner::Protected(inner) => inner.manifest().game_id.clone(),
            }
        }

        pub fn content_root_blake3(&self) -> String {
            match &self.inner {
                WebPakInner::Core(inner) => inner.content_root_hex(),
                WebPakInner::Protected(inner) => inner.content_root().to_owned(),
            }
        }
    }

    fn fixed_key(value: &str) -> Result<[u8; 32], JsValue> {
        let bytes = hex::decode(value)
            .map_err(|error| JsValue::from_str(&format!("invalid 32-byte key: {error}")))?;
        bytes.try_into().map_err(|_| {
            JsValue::from_str("invalid key length; expected exactly 64 hexadecimal characters")
        })
    }

    #[wasm_bindgen]
    #[derive(Debug)]
    pub struct WebRuntime {
        inner: PortableWebRuntime,
    }

    #[wasm_bindgen]
    impl WebRuntime {
        #[wasm_bindgen(constructor)]
        pub fn new(ariac: &[u8], logical_width: u32, logical_height: u32) -> Result<Self, JsValue> {
            let program = CompiledProgram::decode(ariac)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let inner = PortableWebRuntime::new(
                program,
                LogicalSize {
                    width: logical_width,
                    height: logical_height,
                },
            )
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(Self { inner })
        }

        pub fn step(&mut self, input_json: &str) -> Result<String, JsValue> {
            let input: InputSnapshot = serde_json::from_str(input_json)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let output = self
                .inner
                .step(&input)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            serde_json::to_string(&output).map_err(|error| JsValue::from_str(&error.to_string()))
        }

        pub fn snapshot_json(&self) -> Result<String, JsValue> {
            serde_json::to_string(&self.inner.snapshot())
                .map_err(|error| JsValue::from_str(&error.to_string()))
        }

        pub fn restore_json(&mut self, snapshot_json: &str) -> Result<(), JsValue> {
            let snapshot: VmSnapshot = serde_json::from_str(snapshot_json)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            self.inner
                .restore(snapshot)
                .map_err(|error| JsValue::from_str(&error.to_string()))
        }

        pub fn save_envelope_json(&self, timestamp_unix_ms: u64) -> Result<String, JsValue> {
            let envelope = self
                .inner
                .save_envelope(timestamp_unix_ms)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            serde_json::to_string(&envelope).map_err(|error| JsValue::from_str(&error.to_string()))
        }

        pub fn restore_envelope_json(&mut self, envelope_json: &str) -> Result<(), JsValue> {
            let envelope = serde_json::from_str(envelope_json)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            self.inner
                .restore_envelope(&envelope)
                .map_err(|error| JsValue::from_str(&error))
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_api::{WebPak, WebRuntime};
