use aria_core::{CompileInput, InputSnapshot, LogicalSize, SkipMode, SourceUnit, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn vm_for(source: &str) -> Vm {
    let output = compile(CompileInput {
        game_id: "jp.example.auto-skip".to_owned(),
        entry: "main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    Vm::new(output.program.unwrap(), SIZE).unwrap()
}

fn tick(vm: &mut Vm, sequence: u64, delta_ms: u32) -> aria_core::StepOutput {
    vm.step(&InputSnapshot::idle(sequence, delta_ms)).unwrap()
}

fn shown(output: &aria_core::StepOutput) -> String {
    output
        .view
        .dialogue
        .as_ref()
        .map(|dialogue| dialogue.full_text.clone())
        .unwrap_or_default()
}

#[test]
fn auto_waits_for_the_reader_after_the_whole_line_is_shown() {
    // Ten graphemes at 100ms each take a second to reveal.
    let mut vm = vm_for(
        r#"aria;
entry start;
scene start {
  text_speed 100;
  narrate "あいうえおかきくけこ";
  await advance;
  narrate "次の行";
  await advance;
  end;
}
"#,
    );
    vm.set_auto_mode(true);
    let mut sequence = 1;
    let mut elapsed = 0;
    // Through the reveal and well past the auto delay counted from the
    // first grapheme, the first line is still on screen and still typing
    // or just completed.
    while elapsed < 1_300 {
        let output = tick(&mut vm, sequence, 100);
        sequence += 1;
        elapsed += 100;
        assert_eq!(
            shown(&output),
            "あいうえおかきくけこ",
            "advanced after {elapsed}ms"
        );
    }
    // Once the auto delay has passed after the line completed, it moves on.
    let mut moved = false;
    for _ in 0..40 {
        let output = tick(&mut vm, sequence, 100);
        sequence += 1;
        if shown(&output) == "次の行" {
            moved = true;
            break;
        }
    }
    assert!(moved, "auto never advanced past a completed line");
}

#[test]
fn a_skip_from_the_menu_passes_authored_holds() {
    let mut vm = vm_for(
        r#"aria;
entry start;
scene start {
  narrate "一行目";
  await advance;
  wait 5000ms;
  narrate "二行目";
  await advance;
  end;
}
"#,
    );
    vm.set_skip_mode(SkipMode::All);
    let mut reached = false;
    // Far fewer than the 5000ms the hold would take at 16ms a frame.
    for sequence in 1..40 {
        let output = tick(&mut vm, sequence, 16);
        if shown(&output) == "二行目" {
            reached = true;
            break;
        }
    }
    assert!(reached, "skip waited out the authored hold");
}

#[test]
fn read_only_skip_turns_off_at_the_first_unread_line() {
    let mut vm = vm_for(
        r#"aria;
entry start;
scene start {
  narrate "まだ読んでいない";
  await advance;
  end;
}
"#,
    );
    vm.set_skip_mode(SkipMode::Read);
    tick(&mut vm, 1, 16);
    let output = tick(&mut vm, 2, 16);
    assert_eq!(shown(&output), "まだ読んでいない");
    assert_eq!(output.view.skip_mode, SkipMode::Off);
}
