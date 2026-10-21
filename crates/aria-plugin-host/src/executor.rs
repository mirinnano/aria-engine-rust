//! Runs plugin calls in Wasm for a VM.
//!
//! Each plugin's module is compiled once. Every call then gets a fresh
//! instance with its own fuel and memory limits, receives the request as
//! JSON in its memory, and returns a JSON `Result`. The guest has no
//! imports, so the request is all it can see.

use std::collections::BTreeMap;

use aria_plugin_sdk::{Capability, PluginExecutor, PluginManifest, PluginRequest, PluginResponse};
use wasmi::{Config, Engine, Linker, Module, Store, StoreLimitsBuilder};

use crate::{MAX_PLUGIN_MEMORY, PluginLoadError};

/// Fuel for one call. Generous for game logic, small enough that a runaway
/// loop stops within a frame.
pub const CALL_FUEL: u64 = 2_000_000;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct WasmExecutor {
    engine: Engine,
    modules: BTreeMap<String, Module>,
}

impl WasmExecutor {
    /// Compiles the guests that need calls at run time (commands, hooks,
    /// state migration). `plugins` pairs each manifest with its Wasm bytes.
    pub fn new<'a>(
        plugins: impl IntoIterator<Item = (&'a PluginManifest, &'a [u8])>,
    ) -> Result<Self, PluginLoadError> {
        let mut config = Config::default();
        config.consume_fuel(true);
        config.enforced_limits(wasmi::EnforcedLimits::strict());
        let engine = Engine::new(&config);
        let mut modules = BTreeMap::new();
        for (manifest, wasm) in plugins {
            if !needs_calls(manifest) {
                continue;
            }
            let module = Module::new(&engine, wasm).map_err(wasm_error)?;
            if module.imports().next().is_some() {
                return Err(PluginLoadError::Imports);
            }
            for export in ["aria_alloc", "aria_call", "memory"] {
                if module.get_export(export).is_none() {
                    return Err(PluginLoadError::Wasm(format!(
                        "plugin '{}' declares calls but does not export {export}",
                        manifest.id
                    )));
                }
            }
            modules.insert(manifest.id.clone(), module);
        }
        Ok(Self { engine, modules })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

/// Whether a plugin is ever called after registration.
#[must_use]
pub fn needs_calls(manifest: &PluginManifest) -> bool {
    manifest.capabilities.contains(&Capability::Commands)
        || manifest.capabilities.contains(&Capability::Hooks)
        || manifest.migrates_from.is_some()
}

impl PluginExecutor for WasmExecutor {
    fn call(&self, plugin_id: &str, request: &PluginRequest) -> Result<PluginResponse, String> {
        let module = self
            .modules
            .get(plugin_id)
            .ok_or_else(|| format!("plugin '{plugin_id}' cannot be called"))?;
        let input = serde_json::to_vec(request).map_err(|error| error.to_string())?;
        if input.len() > MAX_REQUEST_BYTES {
            return Err(format!("request to '{plugin_id}' exceeds 1 MiB"));
        }
        let limits = StoreLimitsBuilder::new()
            .memory_size(MAX_PLUGIN_MEMORY)
            .table_elements(4096)
            .memories(1)
            .tables(1)
            .instances(1)
            .build();
        let mut store = Store::new(&self.engine, limits);
        store.limiter(|limits| limits);
        store
            .set_fuel(CALL_FUEL)
            .map_err(|error| error.to_string())?;
        let fail = |error: wasmi::Error| format!("plugin '{plugin_id}' failed: {error}");
        let instance = Linker::new(&self.engine)
            .instantiate(&mut store, module)
            .map_err(fail)?
            .start(&mut store)
            .map_err(fail)?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| format!("plugin '{plugin_id}' has no memory"))?;
        let pointer = instance
            .get_typed_func::<u32, u32>(&store, "aria_alloc")
            .map_err(fail)?
            .call(&mut store, input.len() as u32)
            .map_err(fail)?;
        memory
            .write(&mut store, pointer as usize, &input)
            .map_err(|error| format!("plugin '{plugin_id}' request did not fit: {error}"))?;
        let packed = instance
            .get_typed_func::<(u32, u32), u64>(&store, "aria_call")
            .map_err(fail)?
            .call(&mut store, (pointer, input.len() as u32))
            .map_err(fail)?;
        let (output_pointer, output_len) =
            ((packed >> 32) as usize, (packed & 0xffff_ffff) as usize);
        if output_len > MAX_RESPONSE_BYTES {
            return Err(format!("response from '{plugin_id}' exceeds 1 MiB"));
        }
        let mut output = vec![0; output_len];
        memory
            .read(&store, output_pointer, &mut output)
            .map_err(|error| format!("plugin '{plugin_id}' response is out of bounds: {error}"))?;
        serde_json::from_slice::<Result<PluginResponse, String>>(&output)
            .map_err(|error| format!("plugin '{plugin_id}' response is malformed: {error}"))?
            .map_err(|message| format!("plugin '{plugin_id}': {message}"))
    }
}

fn wasm_error(error: impl std::fmt::Display) -> PluginLoadError {
    PluginLoadError::Wasm(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_plugin_sdk::{PluginValue, StoryState};

    /// A guest whose `aria_call` returns a fixed response.
    fn guest(manifest: &str, response: &str) -> Vec<u8> {
        let escape = |text: &str| {
            text.bytes()
                .map(|b| format!("\\{b:02x}"))
                .collect::<String>()
        };
        wat::parse_str(format!(
            r#"(module
              (memory (export "memory") 1)
              (func (export "aria_plugin_api_version") (result i32) i32.const 2)
              (func (export "aria_plugin_manifest_ptr") (result i32) i32.const 0)
              (func (export "aria_plugin_manifest_len") (result i32) i32.const {manifest_len})
              (func (export "aria_alloc") (param i32) (result i32) i32.const 8192)
              (func (export "aria_call") (param i32 i32) (result i64)
                i64.const {response_pointer} i64.const 32 i64.shl i64.const {response_len} i64.or)
              (data (i32.const 0) "{manifest}")
              (data (i32.const 4096) "{response}"))"#,
            manifest_len = manifest.len(),
            manifest = escape(manifest),
            response_pointer = 4096,
            response_len = response.len(),
            response = escape(response),
        ))
        .unwrap()
    }

    const MANIFEST: &str = r#"{"api_version":2,"id":"test.score","version":"1.0.0","capabilities":["commands"],"namespace":"score","commands":[{"name":"double","params":["int"],"returns":"int"}]}"#;

    fn request() -> PluginRequest {
        PluginRequest::Command {
            command: "double".into(),
            args: vec![PluginValue::Int(2)],
            state: serde_json::Value::Null,
            story: StoryState::default(),
        }
    }

    #[test]
    fn a_call_returns_the_guest_response() {
        let wasm = guest(MANIFEST, r#"{"Ok":{"result":4}}"#);
        let manifest = crate::load_plugin(&wasm).unwrap().manifest;
        let executor = WasmExecutor::new([(&manifest, wasm.as_slice())]).unwrap();
        let response = executor.call("test.score", &request()).unwrap();
        assert_eq!(response.result, Some(PluginValue::Int(4)));
        let error = guest(MANIFEST, r#"{"Err":"no such command"}"#);
        let executor = WasmExecutor::new([(&manifest, error.as_slice())]).unwrap();
        assert!(
            executor
                .call("test.score", &request())
                .unwrap_err()
                .contains("no such command")
        );
        assert!(executor.call("other", &request()).is_err());
    }

    #[test]
    fn a_runaway_call_runs_out_of_fuel() {
        let escape = |text: &str| {
            text.bytes()
                .map(|b| format!("\\{b:02x}"))
                .collect::<String>()
        };
        let wasm = wat::parse_str(format!(
            r#"(module
              (memory (export "memory") 1)
              (func (export "aria_plugin_api_version") (result i32) i32.const 2)
              (func (export "aria_plugin_manifest_ptr") (result i32) i32.const 0)
              (func (export "aria_plugin_manifest_len") (result i32) i32.const {len})
              (func (export "aria_alloc") (param i32) (result i32) i32.const 8192)
              (func (export "aria_call") (param i32 i32) (result i64)
                (loop $forever br $forever) i64.const 0)
              (data (i32.const 0) "{manifest}"))"#,
            len = MANIFEST.len(),
            manifest = escape(MANIFEST),
        ))
        .unwrap();
        let manifest = crate::load_plugin(&wasm).unwrap().manifest;
        let executor = WasmExecutor::new([(&manifest, wasm.as_slice())]).unwrap();
        assert!(
            executor
                .call("test.score", &request())
                .unwrap_err()
                .contains("failed")
        );
    }

    #[test]
    fn a_guest_without_call_exports_is_refused_when_it_declares_commands() {
        let escape = |text: &str| {
            text.bytes()
                .map(|b| format!("\\{b:02x}"))
                .collect::<String>()
        };
        let wasm = wat::parse_str(format!(
            r#"(module
              (memory (export "memory") 1)
              (func (export "aria_plugin_api_version") (result i32) i32.const 2)
              (func (export "aria_plugin_manifest_ptr") (result i32) i32.const 0)
              (func (export "aria_plugin_manifest_len") (result i32) i32.const {len})
              (data (i32.const 0) "{manifest}"))"#,
            len = MANIFEST.len(),
            manifest = escape(MANIFEST),
        ))
        .unwrap();
        let manifest = crate::load_plugin(&wasm).unwrap().manifest;
        assert!(WasmExecutor::new([(&manifest, wasm.as_slice())]).is_err());
    }
}
