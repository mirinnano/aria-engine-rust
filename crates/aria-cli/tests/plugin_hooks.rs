//! Plugin hooks end to end: a Rust guest that rewrites lines, edits choices,
//! and counts scene visits without the script calling it.
use std::sync::Arc;

use aria_core::compiler::{CompileInput, compile_with_plugins};
use aria_core::protocol::LogicalSize;
use aria_core::{CompiledProgram, InputSnapshot, PluginSet, SourceUnit, UiIntent, Vm};
use aria_plugin_host::executor::WasmExecutor;

const ANNOTATE: &[u8] = include_bytes!("../../../examples/plugins/annotate/assets/annotate.wasm");
const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn program(script: &str) -> CompiledProgram {
    let manifest = aria_plugin_host::load_plugin(ANNOTATE).unwrap().manifest;
    let output = compile_with_plugins(
        CompileInput {
            game_id: "org.example.hook-test".into(),
            entry: "main.aria".into(),
            sources: vec![SourceUnit {
                logical_path: "main.aria".into(),
                source: script.into(),
            }],
        },
        &[manifest],
    );
    assert!(!output.has_errors(), "{:#?}", output.diagnostics);
    output.program.unwrap()
}

fn vm(script: &str) -> Vm {
    let registration = aria_plugin_host::load_plugin(ANNOTATE).unwrap();
    let executor = WasmExecutor::new([(&registration.manifest, ANNOTATE)]).unwrap();
    let set = PluginSet {
        registrations: vec![registration],
        executor: Some(Arc::new(executor)),
    };
    Vm::with_plugin_set(program(script), SIZE, &set).unwrap()
}

fn idle(sequence: u64) -> InputSnapshot {
    InputSnapshot::idle(sequence, 1_000)
}

fn activate(sequence: u64, id: &str) -> InputSnapshot {
    let mut input = InputSnapshot::idle(sequence, 0);
    input.intents.push(UiIntent::Activate { id: id.into() });
    input
}

#[test]
fn a_text_hook_rewrites_the_line_but_not_its_read_key() {
    let mut vm =
        vm(r#"aria; entry start; scene start { narrate "海が見えた。"; await advance; end; }"#);
    let mut output = vm.step(&idle(1)).unwrap();
    for sequence in 2..200 {
        if output
            .view
            .dialogue
            .as_ref()
            .is_some_and(|dialogue| dialogue.complete)
        {
            break;
        }
        output = vm.step(&idle(sequence)).unwrap();
    }
    let dialogue = output.view.dialogue.unwrap();
    // The markup the plugin added is parsed like authored markup.
    assert_eq!(dialogue.text, "海が見えた。");
    assert!(
        serde_json::to_string(&dialogue).unwrap().contains("うみ"),
        "the reading reaches the view model"
    );
    let key = vm.snapshot().text.line_key.unwrap();
    assert!(key.starts_with("start:"), "{key}");
}

#[test]
fn a_choice_hook_hides_adds_and_follows_story_flags() {
    let script = r#"aria;
entry start;
scene start {
    choice {
        "鍵のない扉" => door;
        "廊下へ" => hall;
    }
}
scene door { narrate "扉が開いた。"; await advance; end; }
scene hall {
    flag "has_key" = on;
    choice {
        "鍵のない扉" => door;
        "廊下へ" => hall;
    }
}
"#;
    let mut vm = vm(script);
    let output = vm.step(&idle(1)).unwrap();
    let labels = |output: &aria_core::StepOutput| {
        output
            .view
            .choices
            .iter()
            .map(|choice| choice.label.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&output), ["廊下へ", "最初から"]);
    let to_hall = output.view.choices[0].id.clone();
    let output = vm.step(&activate(2, &to_hall)).unwrap();
    assert_eq!(labels(&output), ["鍵のない扉", "廊下へ", "最初から"]);
}

#[test]
fn a_scene_enter_hook_counts_visits_into_the_script_state() {
    let script = r#"aria;
entry start;
state mut visits: Int = 0;
scene start { jump second; }
scene second { narrate "{visits}"; await advance; end; }
"#;
    let mut vm = vm(script);
    let mut output = vm.step(&idle(1)).unwrap();
    for sequence in 2..200 {
        if output
            .view
            .dialogue
            .as_ref()
            .is_some_and(|dialogue| dialogue.complete)
        {
            break;
        }
        output = vm.step(&idle(sequence)).unwrap();
    }
    assert_eq!(output.view.dialogue.unwrap().text, "2");
    assert_eq!(
        vm.snapshot().plugin_state["org.example.annotate"],
        serde_json::json!({ "scenes": 2 })
    );
    // The plugin's layer is drawn over the scene and kept in the save.
    let label = output
        .scene
        .commands
        .iter()
        .find_map(|command| match command {
            aria_core::protocol::DrawCommand::Text { id, text, .. }
                if id.starts_with("plugin:org.example.annotate:") =>
            {
                Some(text.clone())
            }
            _ => None,
        });
    assert_eq!(label.as_deref(), Some("2"));
    assert_eq!(vm.snapshot().plugin_layers["org.example.annotate"].len(), 2);
}

#[test]
fn hooks_give_the_same_frames_on_native_and_web() {
    let script = r#"aria; entry start; scene start { narrate "海と空。"; await advance; choice { "鍵のない扉" => start; "進む" => start; } }"#;
    let mut native = vm(script);
    let mut web = aria_web::PortableWebRuntime::new(program(script), SIZE).unwrap();
    let secret = [6; 32];
    let public = aria_protection::PakSigningKey::from_bytes("publisher", secret)
        .unwrap()
        .verifying_key_bytes();
    web.add_plugin_candidate(
        "plugins/annotate.ariaplugin",
        aria_plugin_host::package::pack(ANNOTATE, &Default::default(), "publisher", &secret)
            .unwrap(),
    );
    web.install_plugins(&aria_core::project::PluginsManifest {
        trusted_keys: [("publisher".to_owned(), hex::encode(public))].into(),
        require: Default::default(),
    })
    .unwrap();
    for sequence in 1..60 {
        let input = if sequence == 30 {
            activate(sequence, "dialogue.advance")
        } else {
            idle(sequence)
        };
        assert_eq!(
            native.step(&input).unwrap().digest(),
            web.step(&input).unwrap().digest(),
            "frame {sequence}"
        );
    }
}
