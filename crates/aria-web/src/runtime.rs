use std::collections::BTreeMap;

use aria_core::project::PluginsManifest;
use aria_core::protocol::{LogicalSize, StepOutput};
use aria_core::{
    CompiledProgram, InputSnapshot, SaveEnvelopeError, SaveEnvelopeV3, Vm, VmError, VmSnapshot,
};
use aria_plugin_host::package::{PluginPackage, open_package};
use aria_plugin_host::select::{Candidate, select};
use serde::Serialize;

/// Host-testable Web runtime. The wasm-bindgen API is a thin serialization
/// wrapper around this exact implementation.
#[derive(Debug)]
pub struct PortableWebRuntime {
    vm: Vm,
    /// Raw `.ariaplugin` files handed over by the page, verified on install.
    candidates: Vec<(String, Vec<u8>)>,
    /// Installed packages by plugin id, kept for their Web UI modules.
    installed: BTreeMap<String, PluginPackage>,
}

/// What `install_plugins` did, for the page to log or show.
#[derive(Debug, Serialize)]
pub struct PluginInstallReport {
    /// `id@version` of every installed plugin.
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
    pub missing: Vec<String>,
}

impl PortableWebRuntime {
    pub fn new(program: CompiledProgram, logical_size: LogicalSize) -> Result<Self, VmError> {
        Ok(Self {
            vm: Vm::new(program, logical_size)?,
            candidates: Vec::new(),
            installed: BTreeMap::new(),
        })
    }

    pub fn from_ariac(ariac: &[u8], logical_size: LogicalSize) -> Result<Self, String> {
        let program = CompiledProgram::decode(ariac).map_err(|error| error.to_string())?;
        Self::new(program, logical_size).map_err(|error| error.to_string())
    }

    /// Hands over one `.ariaplugin` file the page fetched (the bundle's
    /// `plugins/index.json` lists them). Nothing is trusted until install.
    pub fn add_plugin_candidate(&mut self, source: &str, bytes: Vec<u8>) {
        self.candidates.push((source.to_owned(), bytes));
    }

    /// Verifies every candidate against the bundle's `plugins` table, keeps
    /// the newest acceptable build of each plugin, and registers them before
    /// the first step. Fails when a required plugin is missing.
    pub fn install_plugins(
        &mut self,
        manifest: &PluginsManifest,
    ) -> Result<PluginInstallReport, String> {
        let trusted = manifest.trusted_key_bytes();
        let candidates = std::mem::take(&mut self.candidates)
            .into_iter()
            .map(|(source, bytes)| Candidate {
                source,
                package: open_package(&bytes, &trusted),
            })
            .collect();
        let require = manifest
            .require
            .iter()
            .map(|(id, range)| {
                semver::VersionReq::parse(range)
                    .map(|range| (id.clone(), range))
                    .map_err(|error| format!("plugins.require.{id}: {error}"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let selection = select(candidates, &require);
        if !selection.is_complete() {
            return Err(format!(
                "required plugins are missing: {}",
                selection.missing.join(", ")
            ));
        }
        let mut installed = Vec::new();
        for package in selection.plugins {
            let manifest = &package.registration.manifest;
            installed.push(format!("{}@{}", manifest.id, manifest.version));
            self.vm
                .register_plugin(package.registration.clone())
                .map_err(|error| error.to_string())?;
            self.installed.insert(manifest.id.clone(), package);
        }
        Ok(PluginInstallReport {
            installed,
            skipped: selection.skipped,
            missing: selection.missing,
        })
    }

    /// The Web UI module (`ui.js`) an installed plugin ships, if any.
    #[must_use]
    pub fn plugin_ui_module(&self, plugin_id: &str) -> Option<&[u8]> {
        self.installed.get(plugin_id)?.ui_module()
    }

    pub fn step(&mut self, input: &InputSnapshot) -> Result<StepOutput, VmError> {
        self.vm.step(input)
    }

    #[must_use]
    pub fn snapshot(&self) -> VmSnapshot {
        self.vm.snapshot()
    }

    pub fn restore(&mut self, snapshot: VmSnapshot) -> Result<(), VmError> {
        self.vm.restore(snapshot)
    }

    pub fn save_envelope(
        &self,
        timestamp_unix_ms: u64,
    ) -> Result<SaveEnvelopeV3, SaveEnvelopeError> {
        let snapshot = self.vm.snapshot();
        SaveEnvelopeV3::new(
            snapshot.game_id.clone(),
            aria_core::ENGINE_VERSION,
            timestamp_unix_ms,
            &snapshot,
        )
    }

    pub fn restore_envelope(&mut self, envelope: &SaveEnvelopeV3) -> Result<(), String> {
        let snapshot: VmSnapshot = envelope.payload_as().map_err(|error| error.to_string())?;
        envelope
            .validate_for_game(&snapshot.game_id)
            .map_err(|error| error.to_string())?;
        self.vm.restore(snapshot).map_err(|error| error.to_string())
    }

    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.vm.is_halted()
    }
}
