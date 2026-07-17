use aria_core::protocol::{LogicalSize, StepOutput};
use aria_core::{
    CompiledProgram, InputSnapshot, SaveEnvelopeError, SaveEnvelopeV3, Vm, VmError, VmSnapshot,
};

/// Host-testable Web runtime. The wasm-bindgen API is a thin serialization
/// wrapper around this exact implementation.
#[derive(Debug)]
pub struct PortableWebRuntime {
    vm: Vm,
}

impl PortableWebRuntime {
    pub fn new(program: CompiledProgram, logical_size: LogicalSize) -> Result<Self, VmError> {
        Ok(Self {
            vm: Vm::new(program, logical_size)?,
        })
    }

    pub fn from_ariac(ariac: &[u8], logical_size: LogicalSize) -> Result<Self, String> {
        let program = CompiledProgram::decode(ariac).map_err(|error| error.to_string())?;
        Self::new(program, logical_size).map_err(|error| error.to_string())
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
