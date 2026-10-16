//! Real Rust-built Wasm guests, not only a mock manifest or a WAT fixture.
use aria_core::{
    CompileInput, CompiledProgram, InputSnapshot, LogicalSize, SourceUnit, UiIntent, UiRoute, Vm,
    VmError, compile_with_routes,
};
use aria_native::{ReplayRunner, ReplayTape};
use aria_web::PortableWebRuntime;

const LECTURE: &[u8] = include_bytes!("../../../examples/plugins/lecture/assets/lecture.wasm");

/// Signs the lecture guest and installs it the way a Web page does.
fn install_lecture(web: &mut PortableWebRuntime) -> Result<(), String> {
    let secret = [7; 32];
    let public = aria_protection::PakSigningKey::from_bytes("publisher", secret)
        .unwrap()
        .verifying_key_bytes();
    let package =
        aria_plugin_host::package::pack(LECTURE, &Default::default(), "publisher", &secret)
            .unwrap();
    web.add_plugin_candidate("plugins/org.example.lecture.ariaplugin", package);
    let manifest = aria_core::project::PluginsManifest {
        trusted_keys: [("publisher".to_owned(), hex::encode(public))].into(),
        require: [("org.example.lecture".to_owned(), "^1".to_owned())].into(),
    };
    web.install_plugins(&manifest).map(|_| ())
}
const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn program() -> CompiledProgram {
    let result = compile_with_routes(CompileInput {
        game_id: "org.example.plugin-test".into(), entry: "main.aria".into(),
        sources: vec![SourceUnit { logical_path: "main.aria".into(), source:
            "aria; entry start; scene start { screen lecture_slide; narrate \"一枚のスライド。\"; wait 900ms; screen dialogue; narrate \"通常の本文。\"; await advance; end; }".into() }],
    }, &["lecture_slide".to_owned()].into());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.program.unwrap()
}

fn action(sequence: u64, id: &str) -> InputSnapshot {
    let mut input = InputSnapshot::idle(sequence, 0);
    input.intents.push(UiIntent::Activate { id: id.into() });
    input
}

fn custom_program(body: &str) -> CompiledProgram {
    let result = compile_with_routes(
        CompileInput {
            game_id: "org.example.plugin-test".into(),
            entry: "main.aria".into(),
            sources: vec![SourceUnit {
                logical_path: "main.aria".into(),
                source: format!(
                    "aria; entry start; scene start {{ screen lecture_slide; {body} screen dialogue; narrate \"次の本文。\"; await advance; end; }}"
                ),
            }],
        },
        &["lecture_slide".to_owned()].into(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.program.unwrap()
}

#[test]
fn registration_lock_cannot_be_reopened_by_restore_or_invalid_input() {
    let lecture = aria_plugin_host::load_plugin(LECTURE).unwrap();
    let mut vm = Vm::with_plugins(program(), SIZE, std::slice::from_ref(&lecture)).unwrap();
    let initial = vm.snapshot();
    vm.step(&InputSnapshot::idle(1, 0)).unwrap();
    vm.restore(initial).unwrap();
    assert!(matches!(
        vm.register_plugin(aria_plugin_host::load_plugin(LECTURE).unwrap()),
        Err(VmError::InvalidPlugin(_))
    ));
    let mut fresh = Vm::new(program(), SIZE).unwrap();
    let mut invalid = InputSnapshot::idle(1, 0);
    invalid.scroll_delta_y = f32::NAN;
    assert!(fresh.step(&invalid).is_err());
    assert!(matches!(
        fresh.register_plugin(lecture),
        Err(VmError::InvalidPlugin(_))
    ));
}

#[test]
fn semantic_and_direct_advance_share_hard_and_soft_hold_policy() {
    let mut plugin = aria_plugin_host::load_plugin(LECTURE).unwrap();
    plugin.manifest.routes[0].advance_action = None;
    for wait in ["wait 900ms;", "wait breath 300ms;"] {
        let program = custom_program(&format!("narrate \"スライド。\"; {wait}"));
        let mut direct =
            Vm::with_plugins(program.clone(), SIZE, std::slice::from_ref(&plugin)).unwrap();
        let mut semantic = Vm::with_plugins(program, SIZE, std::slice::from_ref(&plugin)).unwrap();
        direct.step(&InputSnapshot::idle(1, 0)).unwrap();
        semantic.step(&InputSnapshot::idle(1, 0)).unwrap();
        let mut input = InputSnapshot::idle(2, 0);
        input.pressed.insert(aria_core::InputAction::Advance);
        let d = direct.step(&input).unwrap();
        let s = semantic.step(&action(2, "dialogue.advance")).unwrap();
        assert_eq!(d.view.route, UiRoute::Dialogue);
        assert_eq!(d.digest(), s.digest());
        assert_eq!(direct.snapshot(), semantic.snapshot());
    }
}

#[test]
fn custom_advance_handles_await_and_reveals_every_atomic_page() {
    let plugin = aria_plugin_host::load_plugin(LECTURE).unwrap();
    let program = custom_program(&format!(
        "narrate \"{}\"; await advance;",
        "長い説明文。".repeat(50)
    ));
    let mut vm = Vm::with_plugins(program.clone(), SIZE, &[plugin]).unwrap();
    let mut web = PortableWebRuntime::new(program, SIZE).unwrap();
    install_lecture(&mut web).unwrap();
    let mut output = vm.step(&InputSnapshot::idle(1, 0)).unwrap();
    assert_eq!(
        output.digest(),
        web.step(&InputSnapshot::idle(1, 0)).unwrap().digest()
    );
    assert!(output.view.dialogue.as_ref().unwrap().page_count > 1);
    for sequence in 2..50 {
        if output.view.route == UiRoute::Dialogue {
            return;
        }
        assert!(output.view.dialogue.as_ref().unwrap().complete);
        let input = action(sequence, "lecture.next");
        output = vm.step(&input).unwrap();
        assert_eq!(output.digest(), web.step(&input).unwrap().digest());
    }
    panic!("custom await advance never reached the next surface");
}

#[test]
fn independent_rust_guest_has_native_web_replay_and_save_parity() {
    let plugin = aria_plugin_host::load_plugin(LECTURE).unwrap();
    assert_eq!(plugin.manifest.id, "org.example.lecture");
    let mut native = Vm::with_plugins(program(), SIZE, std::slice::from_ref(&plugin)).unwrap();
    let mut web = PortableWebRuntime::new(program(), SIZE).unwrap();
    install_lecture(&mut web).unwrap();
    let inputs = vec![
        InputSnapshot::idle(1, 0),
        InputSnapshot::idle(2, 100),
        action(3, "lecture.next"),
        action(4, "dialogue.advance"),
    ];
    for input in &inputs {
        let n = native.step(input).unwrap();
        let w = web.step(input).unwrap();
        assert_eq!(n.digest(), w.digest());
        if input.sequence == 1 {
            assert_eq!(n.view.route, UiRoute::Custom("lecture_slide".into()));
            assert!(n.view.dialogue.unwrap().complete);
            assert!(
                n.view
                    .actions
                    .iter()
                    .any(|action| action.id == "lecture.next")
            );
        }
        if input.sequence == 2 {
            let save = native.snapshot();
            let mut restored =
                Vm::with_plugins(program(), SIZE, std::slice::from_ref(&plugin)).unwrap();
            restored.restore(save.clone()).unwrap();
            assert_eq!(restored.snapshot().plugins, save.plugins);
            let mut missing = Vm::new(program(), SIZE).unwrap();
            assert!(matches!(
                missing.restore(save.clone()),
                Err(VmError::PluginMismatch(_))
            ));
            // A new build of the plugin with the same state version still
            // opens the save; only a changed state version refuses it.
            let mut updated = plugin.clone();
            updated.wasm_blake3 = "f".repeat(64);
            updated.manifest.version = "1.1.0".into();
            let mut later = Vm::with_plugins(program(), SIZE, &[updated.clone()]).unwrap();
            later.restore(save.clone()).unwrap();
            updated.manifest.state_version += 1;
            let mut incompatible = Vm::with_plugins(program(), SIZE, &[updated]).unwrap();
            assert!(matches!(
                incompatible.restore(save),
                Err(VmError::PluginMismatch(_))
            ));
        }
    }
    let tape = ReplayTape {
        name: "independent-plugin".into(),
        viewport: None,
        inputs,
    };
    let (_, replay) = ReplayRunner
        .run_with_plugins(program(), SIZE, &tape, &[plugin])
        .unwrap();
    assert_eq!(replay, native.snapshot());
    assert_eq!(web.snapshot(), native.snapshot());
    assert!(
        install_lecture(&mut web)
            .unwrap_err()
            .contains("before the first step")
    );
}

#[test]
fn missing_plugin_fails_closed() {
    let mut missing = Vm::new(program(), SIZE).unwrap();
    assert!(
        matches!(missing.step(&InputSnapshot::idle(1, 0)), Err(VmError::InvalidPlugin(message)) if message.contains("lecture_slide"))
    );
}

#[test]
fn entry_limited_package_keeps_plugins_and_native_cli_mounts_them() {
    use aria_cli::build::{
        BuildProfile, BuildTarget, BundleManifest, command_with_profile_and_runtime_overrides,
    };
    use std::{fs, process::Command};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("game");
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::create_dir_all(root.join("assets/fonts")).unwrap();
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/v3-minimal/assets/fonts/NotoSansJP-Regular.ttf"
        ),
        root.join("assets/fonts/NotoSansJP-Regular.ttf"),
    )
    .unwrap();
    let secret = [7; 32];
    let public = aria_protection::PakSigningKey::from_bytes("publisher", secret)
        .unwrap()
        .verifying_key_bytes();
    fs::create_dir_all(root.join("plugins")).unwrap();
    fs::write(
        root.join("plugins/lecture.ariaplugin"),
        aria_plugin_host::package::pack(LECTURE, &Default::default(), "publisher", &secret)
            .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("aria.toml"),
        format!(
            r#"schema = 6
[game]
id = "org.example.packaged-plugin"
version = "1.0.0"
[runtime]
entry = "scripts/main.aria"
logical_width = 1280
logical_height = 720
asset_roots = ["assets"]
fonts = ["assets/fonts/NotoSansJP-Regular.ttf"]
save_namespace = "packaged-plugin"
[presentation]
frontend = "ui"
[plugins]
trusted_keys = {{ publisher = "{}" }}
require = {{ "org.example.lecture" = "^1" }}
"#,
            hex::encode(public)
        ),
    )
    .unwrap();
    fs::write(root.join("scripts/main.aria"), "aria; entry start; scene start { screen lecture_slide; narrate \"スライド。\"; wait 900ms; end; }").unwrap();
    let out = temp.path().join("package");
    command_with_profile_and_runtime_overrides(
        &root,
        BuildTarget::LinuxX64,
        Some(&out),
        false,
        BuildProfile::Dev,
        None,
        None,
        Some(false),
        None,
        Some("scripts/main.aria"),
        None,
    )
    .unwrap();
    let bundle: BundleManifest =
        serde_json::from_slice(&fs::read(out.join("bundle.aria.json")).unwrap()).unwrap();
    assert_eq!(bundle.schema_version, 7);
    assert_eq!(bundle.plugins.require["org.example.lecture"], "^1");
    // The plugin ships beside the PAK, not inside it, so it can be replaced.
    assert!(out.join("plugins/org.example.lecture.ariaplugin").is_file());
    assert!(
        bundle
            .pak_packs
            .iter()
            .all(|pack| pack.assets.iter().all(|asset| !asset.ends_with(".wasm")))
    );
    let output = Command::new(env!("CARGO_BIN_EXE_aria"))
        .args(["run", out.to_str().unwrap(), "--headless"])
        .env("ARIA_SAVE_DIR", temp.path().join("user"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let step: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(step["view"]["route"]["name"], "lecture_slide");
    assert_eq!(step["view"]["story_surface"]["atomic_text"], true);
}

#[test]
fn plugin_policy_survives_backlog_reconstruction() {
    let plugin = aria_plugin_host::load_plugin(LECTURE).unwrap();
    let mut vm = Vm::with_plugins(program(), SIZE, &[plugin]).unwrap();
    let opening = vm.step(&InputSnapshot::idle(1, 0)).unwrap();
    let page_id = opening.view.dialogue.unwrap().page_id;
    vm.step(&action(2, "lecture.next")).unwrap();
    vm.step(&action(3, "dialogue.advance")).unwrap();
    vm.step(&action(4, "chrome.backlog")).unwrap();
    vm.step(&action(5, &format!("backlog:{page_id}"))).unwrap();
    let resumed = vm.step(&action(6, "confirm.accept")).unwrap();
    assert_eq!(resumed.view.route, UiRoute::Custom("lecture_slide".into()));
    // The presentation learns which plugin owns the screen, to mount its UI.
    assert_eq!(
        resumed.view.route_plugin.as_deref(),
        Some("org.example.lecture")
    );
    assert!(resumed.view.dialogue.unwrap().complete);
    assert!(
        resumed
            .view
            .actions
            .iter()
            .any(|action| action.id == "lecture.next")
    );
}

#[test]
fn a_save_from_the_previous_snapshot_schema_still_opens() {
    let plugin = aria_plugin_host::load_plugin(LECTURE).unwrap();
    let mut vm = Vm::with_plugins(program(), SIZE, std::slice::from_ref(&plugin)).unwrap();
    vm.step(&InputSnapshot::idle(1, 0)).unwrap();
    let mut json = serde_json::to_value(vm.snapshot()).unwrap();
    json["schema_version"] = 10.into();
    json.as_object_mut().unwrap().remove("plugins");
    json["plugin_fingerprints"] = serde_json::json!({ "org.example.lecture": "ab".repeat(32) });
    let old: aria_core::VmSnapshot = serde_json::from_value(json).unwrap();
    let mut restored = Vm::with_plugins(program(), SIZE, &[plugin]).unwrap();
    restored.restore(old).unwrap();
    assert_eq!(restored.snapshot().schema_version, 11);
    assert!(restored.snapshot().plugins.contains_key("org.example.lecture"));
}
