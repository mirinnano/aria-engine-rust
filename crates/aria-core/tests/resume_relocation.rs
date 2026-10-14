use aria_core::protocol::LogicalSize;
use aria_core::{
    CompileInput, InputAction, InputSnapshot, SourceUnit, StepOutput, Vm, VmError, compile,
};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn vm(source: &str) -> Vm {
    let output = compile(CompileInput {
        game_id: "jp.example.resume".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    Vm::new(output.program.unwrap(), SIZE).unwrap()
}

struct Driver {
    sequence: u64,
}

impl Driver {
    fn idle(&mut self, vm: &mut Vm) -> StepOutput {
        self.sequence += 1;
        vm.step(&InputSnapshot::idle(self.sequence, 16)).unwrap()
    }

    fn advance(&mut self, vm: &mut Vm) -> StepOutput {
        self.sequence += 1;
        let mut input = InputSnapshot::idle(self.sequence, 16);
        input.pressed.insert(InputAction::Advance);
        vm.step(&input).unwrap()
    }
}

fn line(output: &StepOutput) -> String {
    output
        .view
        .dialogue
        .as_ref()
        .map(|dialogue| dialogue.full_text.clone())
        .unwrap_or_default()
}

const ORIGINAL: &str = r#"aria;
entry start;
state mut visits: Int = 0;
scene start {
  text_speed 0;
  narrate "一行目。";
  await advance;
  visits += 1;
  narrate "二行目に誤字がある。";
  await advance;
  narrate "三行目。";
  await advance;
  end;
}
"#;

#[test]
fn a_save_resumes_on_the_same_line_after_the_script_is_patched() {
    let mut original = vm(ORIGINAL);
    let mut driver = Driver { sequence: 0 };
    assert_eq!(line(&driver.idle(&mut original)), "一行目。");
    driver.advance(&mut original);
    assert_eq!(line(&driver.advance(&mut original)), "三行目。");
    let save = original.snapshot();
    assert!(save.resume.is_some());

    // A new opening line and a typo fix elsewhere move every address.
    let patched_source = ORIGINAL
        .replace(
            "  narrate \"一行目。\";",
            "  narrate \"追加した冒頭。\";\n  await advance;\n  narrate \"一行目。\";",
        )
        .replace("二行目に誤字がある。", "二行目を直した。");
    let mut patched = vm(&patched_source);
    patched.restore(save).unwrap();
    let mut driver = Driver { sequence: 0 };
    assert_eq!(line(&driver.idle(&mut patched)), "三行目。");
    assert_eq!(
        patched.snapshot().int_registers.get("state:visits"),
        Some(&1)
    );
}

#[test]
fn an_unchanged_program_keeps_the_exact_saved_position() {
    let mut first = vm(ORIGINAL);
    let mut driver = Driver { sequence: 0 };
    driver.idle(&mut first);
    driver.advance(&mut first);
    let save = first.snapshot();
    let mut second = vm(ORIGINAL);
    second.restore(save.clone()).unwrap();
    assert_eq!(second.snapshot().pc, save.pc);
}

#[test]
fn read_state_follows_the_line_not_the_order_it_was_shown() {
    let source = r#"aria;
entry menu;
scene menu {
  choice { "二章" => two; "一章" => one; }
}
scene one { text_speed 0; narrate "一章の冒頭。"; await advance; jump menu; }
scene two { text_speed 0; narrate "二章の冒頭。"; await advance; jump menu; }
"#;
    let mut game = vm(source);
    let mut driver = Driver { sequence: 0 };
    driver.idle(&mut game);
    let pick = |vm: &mut Vm, driver: &mut Driver, id: &str| {
        driver.sequence += 1;
        let mut input = InputSnapshot::idle(driver.sequence, 16);
        input
            .intents
            .push(aria_core::UiIntent::Activate { id: id.to_owned() });
        vm.step(&input).unwrap();
        driver.idle(vm)
    };
    assert_eq!(
        line(&pick(&mut game, &mut driver, "choice:0")),
        "二章の冒頭。"
    );
    driver.advance(&mut game);
    driver.idle(&mut game);
    assert_eq!(
        line(&pick(&mut game, &mut driver, "choice:1")),
        "一章の冒頭。"
    );
    driver.advance(&mut game);
    // Display order once made the first line shown anywhere count as read.
    let read = game.snapshot().read_texts;
    assert_eq!(read.len(), 2, "{read:?}");
    assert!(
        read.iter()
            .all(|key| key.starts_with("one:") || key.starts_with("two:"))
    );
}

#[test]
fn a_save_inside_a_call_returns_to_the_caller_after_relocation() {
    let source = r#"aria;
entry start;
scene start {
  text_speed 0;
  call aside;
  narrate "戻ってきた。";
  await advance;
  end;
}
scene aside {
  narrate "寄り道。";
  await advance;
  return;
}
"#;
    let mut original = vm(source);
    let mut driver = Driver { sequence: 0 };
    assert_eq!(line(&driver.idle(&mut original)), "寄り道。");
    let save = original.snapshot();

    let patched = source.replace(
        "  call aside;",
        "  narrate \"前置き。\";\n  await advance;\n  call aside;",
    );
    let mut game = vm(&patched);
    game.restore(save).unwrap();
    let mut driver = Driver { sequence: 0 };
    assert_eq!(line(&driver.idle(&mut game)), "寄り道。");
    assert_eq!(line(&driver.advance(&mut game)), "戻ってきた。");
}

#[test]
fn a_save_whose_scene_was_removed_is_refused_explicitly() {
    let source = r#"aria;
entry start;
scene start { jump gone; }
scene gone { text_speed 0; narrate "消える場面。"; await advance; end; }
"#;
    let mut original = vm(source);
    let mut driver = Driver { sequence: 0 };
    driver.idle(&mut original);
    let save = original.snapshot();
    let mut patched =
        vm("aria;\nentry start;\nscene start { narrate \"新しい場面。\"; await advance; end; }\n");
    assert!(matches!(
        patched.restore(save),
        Err(VmError::SaveRelocation(_))
    ));
}
