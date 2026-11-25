use aria_core::protocol::LogicalSize;
use aria_core::{
    CompileInput, InputAction, InputSnapshot, SourceUnit, StepOutput, UiIntent, Vm, compile,
};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

const SCRIPT: &str = r##"aria;
entry start;
scene start {
  text_speed 0;
  time evening;
  background asset("#203040");
  narrate "夕方。";
  await advance;
  time night over 1000ms;
  narrate "夜へ。";
  await advance;
  background asset("#101820");
  narrate "場所が変わっても夜。";
  await advance;
  time none;
  narrate "時刻なし。";
  await advance;
  end;
}
"##;

const NO_TIME: &str = r##"aria;
entry start;
scene start {
  text_speed 0;
  background asset("#203040");
  narrate "時刻を使わない。";
  await advance;
  end;
}
"##;

fn vm(source: &str) -> Vm {
    let output = compile(CompileInput {
        game_id: "jp.example.time".to_owned(),
        entry: "main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    Vm::new(output.program.unwrap(), SIZE).unwrap()
}

fn idle(vm: &mut Vm, sequence: u64, delta_ms: u32) -> StepOutput {
    vm.step(&InputSnapshot::idle(sequence, delta_ms)).unwrap()
}

fn advance(vm: &mut Vm, sequence: u64) -> StepOutput {
    vm.step(&InputSnapshot::pressed(sequence, 16, InputAction::Advance))
        .unwrap()
}

fn time(output: &StepOutput) -> Option<(Option<String>, Option<String>, f32)> {
    output
        .view
        .time
        .as_ref()
        .map(|time| (time.name.clone(), time.from.clone(), time.progress))
}

#[test]
fn a_time_of_day_fades_on_the_vm_clock_and_survives_a_new_background() {
    let mut vm = vm(SCRIPT);
    let output = idle(&mut vm, 1, 16);
    assert_eq!(time(&output), Some((Some("evening".to_owned()), None, 1.0)));

    // `time night over 1000ms` starts from the evening and advances only with
    // the VM's own clock, so native and web runtimes agree frame for frame.
    let output = advance(&mut vm, 2);
    assert_eq!(
        time(&output),
        Some((Some("night".to_owned()), Some("evening".to_owned()), 0.0))
    );
    let output = idle(&mut vm, 3, 400);
    let (_, from, progress) = time(&output).unwrap();
    assert_eq!(from.as_deref(), Some("evening"));
    assert!((progress - 0.4).abs() < 1e-6, "{progress}");
    let output = idle(&mut vm, 4, 600);
    assert_eq!(time(&output), Some((Some("night".to_owned()), None, 1.0)));

    // A new background is a new place, not a new time.
    let output = advance(&mut vm, 5);
    assert_eq!(time(&output), Some((Some("night".to_owned()), None, 1.0)));

    // `time none` without `over` clears at once.
    let output = advance(&mut vm, 6);
    assert_eq!(time(&output), None);
}

#[test]
fn reduced_motion_changes_the_time_at_once() {
    let mut vm = vm(SCRIPT);
    let mut input = InputSnapshot::idle(1, 16);
    input.intents.push(UiIntent::ToggleSetting {
        name: "reduced_motion".to_owned(),
    });
    vm.step(&input).unwrap();
    let output = advance(&mut vm, 2);
    assert_eq!(time(&output), Some((Some("night".to_owned()), None, 1.0)));
}

#[test]
fn scripts_without_time_keep_their_views_and_saves_unchanged() {
    let mut vm = vm(NO_TIME);
    let output = idle(&mut vm, 1, 16);
    assert!(output.view.time.is_none());
    let view = serde_json::to_string(&output.view).unwrap();
    assert!(!view.contains("\"time\""), "{view}");
    let snapshot = serde_json::to_string(&vm.snapshot()).unwrap();
    assert!(!snapshot.contains("\"time\""), "{snapshot}");
}

#[test]
fn a_save_from_before_time_existed_restores_without_one() {
    let mut source = vm(SCRIPT);
    idle(&mut source, 1, 16);
    let mut snapshot = serde_json::to_value(source.snapshot()).unwrap();
    // Remove the field wherever the state carries it, as an older runtime
    // would have written the snapshot.
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("time");
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut snapshot);
    let mut restored = vm(SCRIPT);
    restored
        .restore(serde_json::from_value(snapshot).unwrap())
        .unwrap();
    let output = idle(&mut restored, 2, 16);
    assert_eq!(time(&output), None);
}
