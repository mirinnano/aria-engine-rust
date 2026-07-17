use aria_core::protocol::{LogicalSize, stable_digest};
use aria_core::{CompileInput, SourceUnit, compile};
use aria_native::{ReplayRunner, ReplayTape};
use aria_web::PortableWebRuntime;

const SCRIPT: &str = include_str!("../../../examples/v3-minimal/scripts/main.aria");
const INPUTS: &str = include_str!("../../../compatibility/v3/vertical-slice-inputs.json");
// This baseline is for the Aria 3.1 structured vertical slice.  It was
// deliberately re-recorded when that source replaced the alpha 3.0 example;
// ARIAC4 itself must preserve this replay after an encode/decode round trip.
const ARIA_3_1_VERTICAL_SLICE_SNAPSHOT_HASH: &str =
    "6e4a5ba002abba07997cd3ff23ea79ba09b38062851843c22b3764a560f5ef1c";

#[test]
fn native_and_web_replay_hashes_match_the_v3_golden_corpus() {
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
        .inputs
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
        ARIA_3_1_VERTICAL_SLICE_SNAPSHOT_HASH
    );
}
