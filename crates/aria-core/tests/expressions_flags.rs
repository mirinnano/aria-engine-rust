use aria_core::protocol::LogicalSize;
use aria_core::{CompileInput, InputSnapshot, Severity, SourceUnit, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn compile_source(source: &str) -> aria_core::CompileOutput {
    compile(CompileInput {
        game_id: "jp.example.expressions".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    })
}

fn run(source: &str) -> Vm {
    let output = compile_source(source);
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let mut vm = Vm::new(output.program.unwrap(), SIZE).unwrap();
    vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    vm
}

fn shown(vm: &mut Vm) -> String {
    vm.step(&InputSnapshot::idle(2, 16))
        .unwrap()
        .view
        .dialogue
        .map(|dialogue| dialogue.full_text)
        .unwrap_or_default()
}

#[test]
fn assignments_take_arithmetic_and_compound_forms() {
    let vm = run(r#"aria;
entry start;
state mut trust: Int = 3;
state mut doubt: Int = 10;
scene start {
  trust = trust + 4;
  doubt -= trust;
  doubt += trust - 1;
  end;
}
"#);
    let registers = vm.snapshot().int_registers;
    assert_eq!(registers.get("state:trust"), Some(&7));
    assert_eq!(registers.get("state:doubt"), Some(&9));
}

#[test]
fn conditions_read_session_and_persistent_flags() {
    let mut vm = run(r#"aria;
entry start;
scene start {
  text_speed 0;
  persistent flag "cleared_once" = true;
  flag "met_mio" = true;
  if flag("cleared_once") && flag("met_mio") {
    narrate "二周目の気配。";
  } else {
    narrate "はじめて。";
  }
  await advance;
  end;
}
"#);
    assert_eq!(shown(&mut vm), "二周目の気配。");
}

#[test]
fn reading_a_flag_no_script_sets_is_a_warning() {
    let output = compile_source(
        r#"aria;
entry start;
scene start {
  flag "met_mio" = true;
  if flag("met_mi0") { narrate "誤字。"; await advance; }
  end;
}
"#,
    );
    assert!(!output.has_errors());
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == Severity::Warning && diagnostic.message.contains("'met_mi0'")
    }));
}

#[test]
fn arithmetic_on_non_integers_is_rejected() {
    let output = compile_source(
        r#"aria;
entry start;
state mut name: String = "ミオ";
scene start { name = name + 1; end; }
"#,
    );
    assert!(output.has_errors());
}
