use aria_core::protocol::{LogicalSize, stable_digest};
use aria_core::{CompiledProgram, InputSnapshot, UiViewport, Vm, VmError, VmSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayTape {
    pub name: String,
    /// A tape-wide viewport is a compact way to record a stable responsive
    /// branch. Individual inputs can still override it to model a resize or
    /// safe-area change at an exact replay point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<UiViewport>,
    pub inputs: Vec<InputSnapshot>,
}

impl ReplayTape {
    /// Resolves the viewport carried by every input before it reaches a
    /// runtime. Keeping that normalization here means Native and Web consume
    /// byte-for-byte equivalent input values during a replay.
    #[must_use]
    pub fn resolved_inputs(&self) -> Vec<InputSnapshot> {
        self.inputs
            .iter()
            .cloned()
            .map(|mut input| {
                if input.viewport.is_none() {
                    input.viewport = self.viewport;
                }
                input
            })
            .collect()
    }
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
        let inputs = tape.resolved_inputs();
        let mut output_hashes = Vec::with_capacity(inputs.len());
        for input in &inputs {
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

#[cfg(test)]
mod tests {
    use aria_core::{InputSnapshot, UiInsets};

    use super::*;

    #[test]
    fn tape_viewport_applies_until_an_input_records_a_resize() {
        let wide = UiViewport {
            width: 1_280,
            height: 720,
            scale_factor: 1.0,
            safe_area: UiInsets::default(),
        };
        let narrow = UiViewport {
            width: 390,
            height: 844,
            scale_factor: 3.0,
            safe_area: UiInsets::default(),
        };
        let tape = ReplayTape {
            name: "viewport".to_owned(),
            viewport: Some(wide),
            inputs: vec![
                InputSnapshot::idle(1, 16),
                InputSnapshot::idle(2, 16).with_viewport(narrow),
            ],
        };

        let inputs = tape.resolved_inputs();
        assert_eq!(inputs[0].viewport, Some(wide));
        assert_eq!(inputs[1].viewport, Some(narrow));
    }
}
