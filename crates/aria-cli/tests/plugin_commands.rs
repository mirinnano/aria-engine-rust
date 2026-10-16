//! Plugin commands end to end: a real Rust guest called from a script, with
//! its state saved, restored, and migrated, on Native and Web alike.
use std::sync::Arc;

use aria_core::compiler::{CompileInput, compile_with_plugins};
use aria_core::protocol::LogicalSize;
use aria_core::{CompiledProgram, InputSnapshot, PluginSet, SourceUnit, Vm};
use aria_plugin_host::executor::WasmExecutor;
use aria_web::PortableWebRuntime;

const TALLY: &[u8] = include_bytes!("../../../examples/plugins/tally/assets/tally.wasm");
const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

const SCRIPT: &str = r#"aria;
entry start;
state mut visits: Int = 0;
state mut total: Int = 0;
scene start {
    total = tally::add(3);
    total = tally::add(4);
    tally::mark("shore");
    if tally::reached(7) {
        narrate "七に届いた。{total}";
    } else {
        narrate "まだ届かない。";
    }
    await advance;
    total = tally::add(1);
    narrate "{total}";
    await advance;
    end;
}
"#;

fn manifest() -> aria_core::PluginManifest {
    aria_plugin_host::load_plugin(TALLY).unwrap().manifest
}

fn compile(source: &str) -> aria_core::CompileOutput {
    compile_with_plugins(
        CompileInput {
            game_id: "org.example.tally-test".into(),
            entry: "main.aria".into(),
            sources: vec![SourceUnit {
                logical_path: "main.aria".into(),
                source: source.into(),
            }],
        },
        &[manifest()],
    )
}

fn program() -> CompiledProgram {
    let output = compile(SCRIPT);
    assert!(!output.has_errors(), "{:#?}", output.diagnostics);
    output.program.unwrap()
}

fn plugin_set() -> PluginSet {
    let registration = aria_plugin_host::load_plugin(TALLY).unwrap();
    let executor = WasmExecutor::new([(&registration.manifest, TALLY)]).unwrap();
    PluginSet {
        registrations: vec![registration],
        executor: Some(Arc::new(executor)),
    }
}

fn advance(sequence: u64) -> InputSnapshot {
    let mut input = InputSnapshot::idle(sequence, 0);
    input.intents.push(aria_core::UiIntent::Activate {
        id: "dialogue.advance".into(),
    });
    input
}

/// Steps until the page is fully shown, so the next advance moves on.
fn visible_text(vm: &mut Vm, first: u64) -> (String, u64) {
    let mut sequence = first;
    loop {
        let output = vm.step(&InputSnapshot::idle(sequence, 1_000)).unwrap();
        sequence += 1;
        if let Some(dialogue) = output.view.dialogue
            && dialogue.complete
        {
            return (dialogue.text, sequence);
        }
        assert!(sequence < first + 400, "the page never finished");
    }
}

#[test]
fn a_script_calls_the_plugin_and_sees_its_result_flags_and_variables() {
    let mut vm = Vm::with_plugin_set(program(), SIZE, &plugin_set()).unwrap();
    let (text, _) = visible_text(&mut vm, 1);
    assert_eq!(text, "七に届いた。7");
    let snapshot = vm.snapshot();
    assert_eq!(snapshot.flags.get("visited_shore"), Some(&true));
    assert_eq!(snapshot.int_registers.get("state:visits"), Some(&1));
    assert_eq!(
        snapshot.plugin_state["org.example.tally"],
        serde_json::json!({ "total": 7 })
    );
}

#[test]
fn native_and_web_give_the_same_frames() {
    let mut native = Vm::with_plugin_set(program(), SIZE, &plugin_set()).unwrap();
    let mut web = PortableWebRuntime::new(program(), SIZE).unwrap();
    let secret = [5; 32];
    let public = aria_protection::PakSigningKey::from_bytes("publisher", secret)
        .unwrap()
        .verifying_key_bytes();
    web.add_plugin_candidate(
        "plugins/tally.ariaplugin",
        aria_plugin_host::package::pack(TALLY, &Default::default(), "publisher", &secret).unwrap(),
    );
    web.install_plugins(&aria_core::project::PluginsManifest {
        trusted_keys: [("publisher".to_owned(), hex::encode(public))].into(),
        require: [("org.example.tally".to_owned(), "^1".to_owned())].into(),
    })
    .unwrap();
    for sequence in 1..40 {
        let input = if sequence == 20 {
            advance(sequence)
        } else {
            InputSnapshot::idle(sequence, 50)
        };
        assert_eq!(
            native.step(&input).unwrap().digest(),
            web.step(&input).unwrap().digest(),
            "frame {sequence}"
        );
    }
    assert_eq!(native.snapshot(), web.snapshot());
}

#[test]
fn plugin_state_survives_a_save_and_an_older_save_is_migrated() {
    let set = plugin_set();
    let mut vm = Vm::with_plugin_set(program(), SIZE, &set).unwrap();
    let (_, next) = visible_text(&mut vm, 1);
    let save = vm.snapshot();

    let mut restored = Vm::with_plugin_set(program(), SIZE, &set).unwrap();
    restored.restore(save.clone()).unwrap();
    restored.step(&advance(next)).unwrap();
    let (text, _) = visible_text(&mut restored, next + 1);
    assert_eq!(text, "8", "the total carried over from the save");

    // A save written by state version 1 kept {"sum": n}.
    let mut old = save;
    old.plugins
        .get_mut("org.example.tally")
        .unwrap()
        .state_version = 1;
    old.plugin_state
        .insert("org.example.tally".into(), serde_json::json!({ "sum": 5 }));
    let mut migrated = Vm::with_plugin_set(program(), SIZE, &set).unwrap();
    migrated.restore(old).unwrap();
    assert_eq!(
        migrated.snapshot().plugin_state["org.example.tally"],
        serde_json::json!({ "total": 5 })
    );
    assert_eq!(
        migrated.snapshot().plugins["org.example.tally"].state_version,
        2
    );
}

#[test]
fn calls_are_checked_against_the_plugin_at_compile_time() {
    for (source, expected) in [
        ("total = tally::add();", "takes 1 argument"),
        ("total = tally::add(\"three\");", "expects Int"),
        (
            "total = nobody::add(1);",
            "no installed plugin offers the namespace 'nobody'",
        ),
        ("total = tally::subtract(1);", "has no command 'subtract'"),
        ("total = tally::mark(\"x\");", "returns nothing"),
    ] {
        let script =
            format!("aria; entry start; state mut total: Int = 0; scene start {{ {source} end; }}");
        let output = compile(&script);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "{source}: {:#?}",
            output.diagnostics
        );
    }
}

#[test]
fn a_call_without_an_executor_stops_with_a_clear_error() {
    let set = PluginSet {
        executor: None,
        ..plugin_set()
    };
    let mut vm = Vm::with_plugin_set(program(), SIZE, &set).unwrap();
    let error = vm.step(&InputSnapshot::idle(1, 0)).unwrap_err().to_string();
    assert!(error.contains("plugin executor"), "{error}");
}
