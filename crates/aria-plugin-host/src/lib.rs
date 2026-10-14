//! The same bounded, import-free Wasm plugin loader on Native and Web.
//! Guest code runs only at registration. The returned data is validated and
//! the interpreter is dropped before the deterministic story loop begins.
#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

use aria_plugin_sdk::{MAX_MANIFEST_BYTES, PLUGIN_API_VERSION, PluginManifest, PluginRegistration};
use thiserror::Error;
use wasmi::{Config, Engine, Linker, Module, Store, StoreLimitsBuilder};

pub const MAX_PLUGIN_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PLUGIN_MEMORY: usize = 8 * 1024 * 1024;
pub const PLUGIN_FUEL: u64 = 5_000_000;

#[derive(Debug, Error)]
pub enum PluginLoadError {
    #[error("plugin exceeds the 4 MiB binary limit")]
    TooLarge,
    #[error("plugins may not import host functions, memory, or WASI")]
    Imports,
    #[error("plugin API version {0} is unsupported (expected 1)")]
    ApiVersion(u32),
    #[error("plugin manifest exceeds the 64 KiB limit")]
    ManifestSize,
    #[error("invalid Wasm plugin: {0}")]
    Wasm(String),
    #[error("invalid plugin manifest: {0}")]
    Manifest(String),
}

/// Loads bytes supplied by a host from an integrity-checked asset or an
/// explicitly selected local file. This function performs no I/O itself.
pub fn load_plugin(bytes: &[u8]) -> Result<PluginRegistration, PluginLoadError> {
    if bytes.len() > MAX_PLUGIN_BYTES {
        return Err(PluginLoadError::TooLarge);
    }
    let mut config = Config::default();
    config.consume_fuel(true);
    // Fuel starts at execution, not validation/compilation. Bound module
    // structure before Module::new so tiny-function/segment floods are refused.
    config.enforced_limits(wasmi::EnforcedLimits::strict());
    let engine = Engine::new(&config);
    let module = Module::new(&engine, bytes).map_err(wasm_error)?;
    if module.imports().next().is_some() {
        return Err(PluginLoadError::Imports);
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_PLUGIN_MEMORY)
        .table_elements(4096)
        .memories(1)
        .tables(1)
        .instances(1)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store.set_fuel(PLUGIN_FUEL).map_err(wasm_error)?;
    let linker = Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(wasm_error)?
        .start(&mut store)
        .map_err(wasm_error)?;
    let version = instance
        .get_typed_func::<(), u32>(&store, "aria_plugin_api_version")
        .map_err(wasm_error)?
        .call(&mut store, ())
        .map_err(wasm_error)?;
    if version != PLUGIN_API_VERSION {
        return Err(PluginLoadError::ApiVersion(version));
    }
    let pointer = instance
        .get_typed_func::<(), u32>(&store, "aria_plugin_manifest_ptr")
        .map_err(wasm_error)?
        .call(&mut store, ())
        .map_err(wasm_error)?;
    let length = instance
        .get_typed_func::<(), u32>(&store, "aria_plugin_manifest_len")
        .map_err(wasm_error)?
        .call(&mut store, ())
        .map_err(wasm_error)? as usize;
    if length == 0 || length > MAX_MANIFEST_BYTES {
        return Err(PluginLoadError::ManifestSize);
    }
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| PluginLoadError::Wasm("missing exported memory".to_owned()))?;
    let mut manifest_bytes = vec![0; length];
    memory
        .read(&store, pointer as usize, &mut manifest_bytes)
        .map_err(wasm_error)?;
    let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| PluginLoadError::Manifest(error.to_string()))?;
    manifest
        .validate()
        .map_err(|error| PluginLoadError::Manifest(error.to_string()))?;
    Ok(PluginRegistration {
        manifest,
        wasm_blake3: blake3::hash(bytes).to_hex().to_string(),
    })
}

fn wasm_error(error: impl std::fmt::Display) -> PluginLoadError {
    PluginLoadError::Wasm(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn guest(pointer: &str, length: u32, extra: &str, data: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module
            (memory (export "memory") 1)
            (func (export "aria_plugin_api_version") (result i32) i32.const 1)
            (func (export "aria_plugin_manifest_ptr") (result i32) {pointer})
            (func (export "aria_plugin_manifest_len") (result i32) i32.const {length})
            {extra} (data (i32.const 0) "{data}"))"#
        ))
        .unwrap()
    }
    #[test]
    fn validates_contract_and_binds_it_to_guest_bytes() {
        let json =
            r#"{"api_version":1,"id":"test.lecture","version":"1","routes":[{"route":"lecture"}]}"#;
        let escaped = json
            .bytes()
            .map(|byte| format!("\\{byte:02x}"))
            .collect::<String>();
        let bytes = guest("i32.const 0", json.len() as u32, "", &escaped);
        let registration = load_plugin(&bytes).unwrap();
        assert_eq!(registration.manifest.routes[0].route, "lecture");
        assert_eq!(
            registration.wasm_blake3,
            blake3::hash(&bytes).to_hex().to_string()
        );
    }
    #[test]
    fn rejects_imports_malformed_contracts_and_out_of_bounds_reads() {
        let bytes = wat::parse_str(r#"(module (import "env" "clock" (func)))"#).unwrap();
        assert!(matches!(load_plugin(&bytes), Err(PluginLoadError::Imports)));
        assert!(matches!(
            load_plugin(&guest("i32.const 0", 2, "", "{}")),
            Err(PluginLoadError::Manifest(_))
        ));
        assert!(matches!(
            load_plugin(&guest("i32.const -1", 4, "", "")),
            Err(PluginLoadError::Wasm(_))
        ));
        assert!(matches!(
            load_plugin(&guest("i32.const 0", 65_537, "", "")),
            Err(PluginLoadError::ManifestSize)
        ));
    }
    #[test]
    fn module_structure_is_bounded_before_guest_execution() {
        let wasm = wat::parse_str(format!("(module {})", "(func)".repeat(10_001))).unwrap();
        let error = load_plugin(&wasm).unwrap_err().to_string();
        assert!(
            error.contains("limit") && error.contains("functions"),
            "{error}"
        );
    }

    #[test]
    fn runaway_guest_and_oversized_memory_are_bounded() {
        assert!(matches!(
            load_plugin(&guest(
                "(loop $forever br $forever) i32.const 0",
                2,
                "",
                "{}"
            )),
            Err(PluginLoadError::Wasm(_))
        ));
        let bytes = wat::parse_str("(module (memory 129))").unwrap();
        assert!(matches!(load_plugin(&bytes), Err(PluginLoadError::Wasm(_))));
    }
}
