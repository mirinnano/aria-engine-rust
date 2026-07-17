use aria_core::protocol::{LogicalSize, stable_digest};
use aria_core::{CompiledProgram, InputSnapshot, Vm, VmError, VmSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayTape {
    pub name: String,
    pub inputs: Vec<InputSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayResult {
    pub output_hashes: Vec<String>,
    pub final_snapshot_hash: String,
    pub halted: bool,
}

#[derive(Debug, Default)]
pub struct ReplayRunner;

impl ReplayRunner {
    pub fn run(
        &self,
        program: CompiledProgram,
        logical_size: LogicalSize,
        tape: &ReplayTape,
    ) -> Result<(ReplayResult, VmSnapshot), VmError> {
        let mut vm = Vm::new(program, logical_size)?;
        let mut output_hashes = Vec::with_capacity(tape.inputs.len());
        for input in &tape.inputs {
            output_hashes.push(vm.step(input)?.digest());
        }
        let snapshot = vm.snapshot();
        let result = ReplayResult {
            output_hashes,
            final_snapshot_hash: stable_digest(&snapshot),
            halted: vm.is_halted(),
        };
        Ok((result, snapshot))
    }
}
