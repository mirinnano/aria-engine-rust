use aria_core::protocol::{LogicalSize, stable_digest};
use aria_core::{CompileInput, SourceUnit, compile};
use aria_native::{ReplayRunner, ReplayTape};
use aria_web::PortableWebRuntime;

const SCRIPT: &str = include_str!("../../../examples/v3-minimal/scripts/main.aria");
const INPUTS: &str = include_str!("../../../compatibility/v3/vertical-slice-inputs.json");
// The story source remains the structured vertical slice, while its UI now
// travels through ARIAC7 and snapshot schema 9. Deterministic subtitle
// paging, replay targets, and semantic gallery state are part of the current
// single-language surface, even when their collections are empty.
// This baseline is updated only after Native/Web parity and bytecode
// encode/decode equality are asserted above.
const ARIA_SINGLE_LANGUAGE_VERTICAL_SLICE_SNAPSHOT_HASH: &str =
    "35db88d1ec0c4299bf6f5ce1ba8f94a83ad5485c72ccc9ca9aa6bb7ca8124af3";

#[test]
fn native_and_web_replay_hashes_match_the_single_language_golden_corpus() {
    let compiled = compile(CompileInput {
        game_id: "jp.example.aria-v3-minimal".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: SCRIPT.to_owned(),
        }],
    });
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let program = compiled.program.unwrap();
    let tape: ReplayTape = serde_json::from_str(INPUTS).unwrap();
    let size = LogicalSize {
        width: 1280,
        height: 720,
    };

    let (native, native_snapshot) = ReplayRunner.run(program.clone(), size, &tape).unwrap();
    let (ariac4, ariac4_snapshot) = ReplayRunner
        .run(
            aria_core::CompiledProgram::decode(&program.encode().unwrap()).unwrap(),
            size,
            &tape,
        )
        .unwrap();
    let mut web = PortableWebRuntime::new(program, size).unwrap();
    let web_hashes = tape
        .resolved_inputs()
        .iter()
        .map(|input| web.step(input).unwrap().digest())
        .collect::<Vec<_>>();

    assert_eq!(web_hashes, native.output_hashes);
    assert_eq!(stable_digest(&web.snapshot()), native.final_snapshot_hash);
    assert_eq!(web.snapshot(), native_snapshot);
    assert_eq!(ariac4.output_hashes, native.output_hashes);
    assert_eq!(ariac4.final_snapshot_hash, native.final_snapshot_hash);
    assert_eq!(ariac4_snapshot, native_snapshot);
    assert_eq!(
        native.final_snapshot_hash,
        ARIA_SINGLE_LANGUAGE_VERTICAL_SLICE_SNAPSHOT_HASH
    );
}
