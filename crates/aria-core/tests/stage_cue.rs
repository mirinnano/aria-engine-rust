use aria_core::protocol::LogicalSize;
use aria_core::{CompileInput, InputAction, InputSnapshot, SourceUnit, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

const CUED_SCENE: &str = r##"aria;
entry start;
scene start {
  text_speed 0;
  stage ward;
  narrate "一";
  await advance;
  stage "rain-city";
  narrate "二";
  await advance;
  stage none;
  narrate "三";
  await advance;
  stage harbor;
  background asset("#05070b");
  narrate "四";
  await advance;
  end;
}
"##;

fn compile_source(source: &str) -> aria_core::CompileOutput {
    compile(CompileInput {
        game_id: "jp.example.stage".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    })
}

#[test]
fn stage_cue_reaches_the_view_model_and_survives_a_snapshot() {
    let output = compile_source(CUED_SCENE);
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let mut vm = Vm::new(output.program.unwrap(), SIZE).unwrap();
    let mut frame = 1;
    let mut step = |vm: &mut Vm, advance: bool| {
        let mut input = InputSnapshot::idle(frame, 16);
        if advance {
            input.pressed.insert(InputAction::Advance);
        }
        frame += 1;
        vm.step(&input).unwrap()
    };

    assert_eq!(step(&mut vm, false).view.stage.as_deref(), Some("ward"));
    let restored = vm.snapshot();
    let after = step(&mut vm, true);
    assert_eq!(after.view.stage.as_deref(), Some("rain-city"));
    assert_eq!(step(&mut vm, true).view.stage, None);
    assert_eq!(
        step(&mut vm, true).view.stage,
        None,
        "a new background must not inherit the previous cue"
    );

    let mut resumed = Vm::new(compile_source(CUED_SCENE).program.unwrap(), SIZE).unwrap();
    resumed.restore(restored).unwrap();
    assert_eq!(resumed.snapshot().stage.as_deref(), Some("ward"));
}

#[test]
fn stage_cue_rejects_presentation_tokens() {
    for cue in ["\"#264450\"", "\"Ward\"", "\"rgba(0,0,0,1)\""] {
        let output = compile_source(&format!(
            "aria;\nentry start;\nscene start {{\n  stage {cue};\n  end;\n}}\n"
        ));
        assert!(output.has_errors(), "{cue} should be rejected");
    }
}
