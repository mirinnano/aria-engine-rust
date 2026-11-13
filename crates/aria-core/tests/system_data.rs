use aria_core::protocol::LogicalSize;
use aria_core::{
    CompileInput, InputAction, InputSnapshot, SourceUnit, SystemData, UiIntent, Vm, VmError,
    compile,
};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

const SCRIPT: &str = r#"aria;
entry start;
cg shore { title "浜"; }
scene start {
  text_speed 0;
  narrate "一行目。";
  await advance;
  persistent flag "saw_shore" = on;
  unlock cg "shore";
  narrate "二行目。";
  await advance;
  end;
}
"#;

fn new_vm() -> Vm {
    let output = compile(CompileInput {
        game_id: "jp.example.system".to_owned(),
        entry: "main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "main.aria".to_owned(),
            source: SCRIPT.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    Vm::new(output.program.unwrap(), SIZE).unwrap()
}

fn advance(vm: &mut Vm, sequence: u64) {
    vm.step(&InputSnapshot::pressed(sequence, 16, InputAction::Advance))
        .unwrap();
}

fn set_text_speed(vm: &mut Vm, sequence: u64, value: f32) {
    let mut input = InputSnapshot::idle(sequence, 16);
    input.intents.push(UiIntent::SetSetting {
        name: "text_speed_ms".to_owned(),
        value,
    });
    vm.step(&input).unwrap();
}

/// Plays past the first line, the flag, and the CG, ending on line two.
fn play_to_second_line(vm: &mut Vm) {
    vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    advance(vm, 2);
    vm.step(&InputSnapshot::idle(3, 16)).unwrap();
}

#[test]
fn loading_an_earlier_slot_keeps_the_readers_later_progress() {
    let mut vm = new_vm();
    let empty = vm.system_data();
    vm.install_system_data(empty).unwrap();
    vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    let early_slot = vm.snapshot();
    advance(&mut vm, 2);
    vm.step(&InputSnapshot::idle(3, 16)).unwrap();
    set_text_speed(&mut vm, 4, 40.0);
    assert!(vm.snapshot().persistent_flags["saw_shore"]);

    vm.restore(early_slot).unwrap();
    let after = vm.snapshot();
    // The position goes back; what the reader has gained does not.
    assert!(after.persistent_flags["saw_shore"]);
    assert_eq!(after.flags.get("saw_shore"), Some(&true));
    assert!(after.unlocked_cgs.contains("shore"));
    assert!(!after.read_texts.is_empty());
    assert_eq!(after.settings.text_speed_ms, 40);
}

#[test]
fn installed_system_data_survives_a_new_session_and_a_slot_load() {
    let mut first = new_vm();
    play_to_second_line(&mut first);
    set_text_speed(&mut first, 4, 40.0);
    let stored: SystemData =
        serde_json::from_str(&serde_json::to_string(&first.system_data()).unwrap()).unwrap();

    // A fresh session that never saw the flag or the CG.
    let mut fresh = new_vm();
    fresh.install_system_data(stored).unwrap();
    fresh.step(&InputSnapshot::idle(1, 16)).unwrap();
    let slot = fresh.snapshot();
    let mut later = new_vm();
    later.install_system_data(fresh.system_data()).unwrap();
    later.restore(slot).unwrap();
    let state = later.snapshot();
    assert!(state.persistent_flags["saw_shore"]);
    assert!(state.unlocked_cgs.contains("shore"));
    assert_eq!(state.settings.text_speed_ms, 40);
}

#[test]
fn a_host_without_system_data_still_gets_the_slots_settings() {
    let mut vm = new_vm();
    vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    set_text_speed(&mut vm, 2, 40.0);
    let slot = vm.snapshot();

    let mut reopened = new_vm();
    reopened.restore(slot).unwrap();
    assert_eq!(reopened.snapshot().settings.text_speed_ms, 40);
}

#[test]
fn the_revision_moves_only_when_system_data_changes() {
    let mut vm = new_vm();
    let start = vm
        .step(&InputSnapshot::idle(1, 16))
        .unwrap()
        .view
        .system_revision;
    let idle = vm
        .step(&InputSnapshot::idle(2, 16))
        .unwrap()
        .view
        .system_revision;
    assert_eq!(idle, start);
    // Finishing line one marks it read.
    advance(&mut vm, 3);
    let read = vm
        .step(&InputSnapshot::idle(4, 16))
        .unwrap()
        .view
        .system_revision;
    assert!(read > start);
    assert_eq!(read, vm.system_revision());
}

#[test]
fn system_data_from_another_game_is_rejected() {
    let mut vm = new_vm();
    let mut data = vm.system_data();
    data.game_id = "jp.example.other".to_owned();
    assert!(matches!(
        vm.install_system_data(data),
        Err(VmError::WrongGame { .. })
    ));
}

#[test]
fn the_scripts_text_speed_is_a_default_the_reader_can_override() {
    let mut vm = new_vm();
    vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert_eq!(vm.snapshot().settings.text_speed_ms, 0);
    set_text_speed(&mut vm, 2, 40.0);
    let stored = vm.system_data();

    // The next session runs `text_speed 0;` again at the start.
    let mut next = new_vm();
    next.install_system_data(stored).unwrap();
    next.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert_eq!(next.snapshot().settings.text_speed_ms, 40);
}
