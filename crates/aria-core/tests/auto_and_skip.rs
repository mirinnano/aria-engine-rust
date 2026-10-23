use aria_core::{
    CompileInput, InputAction, InputSnapshot, LogicalSize, SkipMode, SourceUnit, Vm, compile,
};

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

#[test]
fn read_only_skip_passes_a_timed_heading_it_has_already_shown() {
    // A heading is narrated and held, never awaited. On a second pass through
    // read text, read-only skip must carry on past it.
    let mut vm = vm_for(
        r#"aria;
entry start;
scene start {
  narrate "九月二十三日";
  wait 1000ms;
  narrate "本文。";
  await advance;
  jump start;
}
"#,
    );
    let mut sequence = 1;
    // First pass: read the heading by waiting, and the line by advancing.
    for _ in 0..40 {
        tick(&mut vm, sequence, 50);
        sequence += 1;
    }
    let mut output = vm
        .step(&InputSnapshot::pressed(sequence, 16, InputAction::Advance))
        .unwrap();
    sequence += 1;
    // Second pass with read-only skip from the heading on.
    vm.set_skip_mode(SkipMode::Read);
    let mut passed = false;
    for _ in 0..80 {
        output = tick(&mut vm, sequence, 16);
        sequence += 1;
        if output.view.skip_mode == SkipMode::Off {
            break;
        }
        if shown(&output) == "本文。" {
            passed = true;
        }
    }
    assert!(passed, "skip stopped at {:?}", shown(&output));
    assert_ne!(
        output.view.skip_mode,
        SkipMode::Off,
        "skip turned off at {:?}",
        shown(&output)
    );
}

#[test]
fn auto_gives_a_long_line_more_time_than_a_short_one() {
    let mut vm = vm_for(
        r#"aria;
entry start;
scene start {
  text_speed 0;
  narrate "短い。";
  await advance;
  narrate "これは読むのに時間がかかる、ずっと長い一行の文章である。";
  await advance;
  narrate "終わり。";
  await advance;
  end;
}
"#,
    );
    vm.set_auto_mode(true);
    let mut sequence = 1;
    let mut shown_for = std::collections::BTreeMap::<String, u32>::new();
    for _ in 0..400 {
        let output = tick(&mut vm, sequence, 50);
        sequence += 1;
        *shown_for.entry(shown(&output)).or_default() += 50;
        if shown(&output) == "終わり。" {
            break;
        }
    }
    let short = shown_for["短い。"];
    let long = shown_for["これは読むのに時間がかかる、ずっと長い一行の文章である。"];
    assert!(long >= short + 1_500, "short {short} ms, long {long} ms");
}
