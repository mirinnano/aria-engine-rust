use aria_core::protocol::{LogicalSize, RuntimeCommand, ScreenEffect};
use aria_core::{CompileInput, InputAction, InputSnapshot, SourceUnit, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

#[test]
fn ownership_aware_ui_state_and_effects_lower_without_host_commands() {
    let source = r##"aria;
entry start;
state route: Int = 0;
scene start {
  locale "ja-JP";
  text_speed 0;
  flag "opened" = true;
  persistent flag "chapter_01" = true;
  background asset("#07131f");
  let mut hero = show rect(0px, 0px, 100px, 100px, "#ffffff") z 1;
  tween &mut hero property "opacity" to 160 over 120ms ease ease_out;
  effect tint "#102030" amount 80 over 120ms;
  preload asset("assets/hero.png");
  screen pause;
  say ミオ: "こんにちは";
  await advance;
  end;
}
"##;
    let output = compile(CompileInput {
        game_id: "jp.example.feature".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let program = output.program.expect("program should be emitted");
    let mut vm = Vm::new(program, SIZE).unwrap();
    let output = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert!(
        output
            .scene
            .effects
            .iter()
            .any(|effect| matches!(effect, ScreenEffect::Tint { .. }))
    );
    assert!(
        output
            .runtime
            .iter()
            .any(|command| matches!(command, RuntimeCommand::PreloadAsset { .. }))
    );
    assert!(
        output
            .view
            .actions
            .iter()
            .any(|action| action.id == "menu.save")
    );
}

#[test]
fn menu_backlog_auto_and_skip_are_deterministic() {
    let source = r##"aria;
entry start;
scene start {
  say A: "一行目";
  await advance;
  say B: "二行目";
  await advance;
  end;
}
"##;
    let output = compile(CompileInput {
        game_id: "jp.example.feature".to_owned(),
        entry: "main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let mut vm = Vm::new(output.program.unwrap(), SIZE).unwrap();
    let first = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert_eq!(
        first
            .view
            .dialogue
            .as_ref()
            .and_then(|dialogue| dialogue.speaker.as_deref()),
        Some("A")
    );
    let mut menu_input = InputSnapshot::pressed(2, 16, InputAction::Menu);
    let menu = vm.step(&menu_input).unwrap();
    assert_eq!(menu.view.route.as_str(), "pause");
    assert!(
        menu.view
            .actions
            .iter()
            .any(|action| action.id == "menu.save")
    );
    menu_input = InputSnapshot::pressed(3, 16, InputAction::Cancel);
    vm.step(&menu_input).unwrap();
    vm.set_auto_mode(true);
    vm.set_skip_mode(aria_core::SkipMode::All);
    let mut sequence = 4;
    for _ in 0..5 {
        let _ = vm.step(&InputSnapshot {
            sequence,
            delta_ms: 1_000,
            pressed: Default::default(),
            held: Default::default(),
            pointer: None,
            scroll_delta_y: 0.0,
            viewport: None,
            intents: Vec::new(),
        });
        sequence += 1;
        if vm.is_halted() {
            break;
        }
    }
    assert!(!vm.backlog().is_empty());
    assert!(vm.read_rate() >= 0.5);
}

#[test]
fn pre_current_visual_ui_saves_are_rejected() {
    let program = aria_core::CompiledProgram::empty("jp.example.feature");
    let mut vm = Vm::new(program, SIZE).unwrap();
    let mut old = serde_json::to_value(vm.snapshot()).unwrap();
    old["schema_version"] = serde_json::json!(3);
    for field in [
        "flags",
        "persistent_flags",
        "chapters",
        "unlocked_cgs",
        "locale",
        "read_texts",
        "backlog",
        "backlog_focused",
        "auto_mode",
        "skip_mode",
        "auto_elapsed_ms",
        "auto_delay_ms",
        "textbox",
        "settings",
        "theme",
        "menu",
        "tweens",
        "effects",
    ] {
        old.as_object_mut().unwrap().remove(field);
    }
    let old: aria_core::VmSnapshot = serde_json::from_value(old).unwrap();
    assert!(matches!(
        vm.restore(old),
        Err(aria_core::VmError::UnsupportedSnapshot(3))
    ));
}
