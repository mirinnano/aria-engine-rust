use aria_core::protocol::LogicalSize;
use aria_core::{CompileInput, InputSnapshot, SourceUnit, UiIntent, UiRoute, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn vm(source: &str) -> Vm {
    let output = compile(CompileInput {
        game_id: "jp.example.cancel".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    Vm::new(output.program.unwrap(), SIZE).unwrap()
}

fn dismiss(vm: &mut Vm, sequence: u64) -> aria_core::StepOutput {
    let mut input = InputSnapshot::idle(sequence, 16);
    input.intents.push(UiIntent::Dismiss);
    vm.step(&input).unwrap()
}

#[test]
fn backing_out_of_a_scripted_screen_follows_its_cancel_target() {
    let mut vm = vm(r#"aria;
entry title;
scene title {
  screen title;
  choice { "はじめる" => chapters; }
}
scene chapters {
  screen chapter_select;
  choice {
    "一章" => one;
    cancel => title;
  }
}
scene one { narrate "一"; await advance; end; }
"#);
    let _ = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    let mut start = InputSnapshot::idle(2, 16);
    start.intents.push(UiIntent::Activate {
        id: "choice:0".to_owned(),
    });
    let catalogue = vm.step(&start).unwrap();
    assert_eq!(catalogue.view.route, UiRoute::ChapterSelect);
    assert_eq!(catalogue.view.choices.len(), 1, "cancel is never shown");

    // Like any choice, the target scene runs on the following step.
    let _ = dismiss(&mut vm, 3);
    let back = vm.step(&InputSnapshot::idle(4, 16)).unwrap();
    assert_eq!(back.view.route, UiRoute::Title);
    assert_eq!(back.view.choices[0].label, "はじめる");
}

#[test]
fn a_scripted_screen_without_cancel_stays_instead_of_orphaning_its_choice() {
    let mut vm = vm(r#"aria;
entry chapters;
scene chapters {
  screen chapter_select;
  choice { "一章" => one; }
}
scene one { narrate "一"; await advance; end; }
"#);
    let catalogue = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert_eq!(catalogue.view.route, UiRoute::ChapterSelect);
    let after = dismiss(&mut vm, 2);
    assert_eq!(after.view.route, UiRoute::ChapterSelect);
    assert_eq!(after.view.choices.len(), 1);
}
