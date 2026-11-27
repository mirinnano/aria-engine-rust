use aria_core::protocol::{AudioBus, LogicalSize};
use aria_core::{AudioCommand, CompileInput, InputSnapshot, SourceUnit, UiIntent, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn vm(source: &str) -> Vm {
    let output = compile(CompileInput {
        game_id: "jp.example.staging".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    Vm::new(output.program.unwrap(), SIZE).unwrap()
}

#[test]
fn ambient_loops_play_on_their_own_bus() {
    let mut game = vm(r#"aria;
entry start;
scene start {
  play bgm asset("assets/bgm/theme.ogg") loop;
  play ambient asset("assets/ambient/rain.ogg") loop fade 800ms;
  play ambient asset("assets/ambient/waves.ogg") loop;
  narrate "雨。";
  await advance;
  end;
}
"#);
    let step = game.step(&InputSnapshot::idle(1, 16)).unwrap();
    let ambient: Vec<_> = step
        .audio
        .iter()
        .filter_map(|command| match command {
            AudioCommand::Play {
                bus: AudioBus::Ambient,
                asset,
                ..
            } => Some(asset.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ambient,
        ["assets/ambient/rain.ogg", "assets/ambient/waves.ogg"]
    );
}

#[test]
fn camera_targets_reach_the_view_and_reset_with_the_backdrop() {
    let mut game = vm(r##"aria;
entry start;
scene start {
  text_speed 0;
  background asset("#102030");
  camera zoom 108 pan (0, -3) over 9000ms ease ease_out;
  narrate "寄る。";
  await advance;
  background asset("#203040");
  narrate "次の場所。";
  await advance;
  end;
}
"##);
    let first = game.step(&InputSnapshot::idle(1, 16)).unwrap();
    let camera = first.view.camera.expect("camera target");
    assert_eq!(
        (camera.zoom, camera.pan_x, camera.pan_y, camera.duration_ms),
        (108, 0, -3, 9000)
    );
    assert_eq!(camera.easing, "ease_out");

    let mut advance = InputSnapshot::idle(2, 16);
    advance.pressed.insert(aria_core::InputAction::Advance);
    let next = game.step(&advance).unwrap();
    assert!(next.view.camera.is_none(), "a new backdrop starts unmoved");
}

#[test]
fn page_settings_merge_survive_the_backdrop_and_reset() {
    let mut game = vm(r##"aria;
entry start;
scene start {
  text_speed 0;
  background asset("#102030");
  page from middle at 45 tracking 80;
  page leading 210 measure 24 size 120 align center;
  narrate "一行。";
  await advance;
  background asset("#203040");
  page tracking -20;
  narrate "次の場所。";
  await advance;
  page reset;
  narrate "元に戻る。";
  await advance;
  end;
}
"##);
    let mut advance = InputSnapshot::idle(1, 16);
    let first = game.step(&advance).unwrap();
    let page = first.view.page.expect("page settings reach the view");
    assert_eq!(page.from.as_deref(), Some("middle"));
    assert_eq!(
        (
            page.at,
            page.tracking,
            page.leading,
            page.measure,
            page.size
        ),
        (Some(45), Some(80), Some(210), Some(24), Some(120))
    );
    assert_eq!(page.align.as_deref(), Some("center"));

    advance.sequence = 2;
    advance.pressed.insert(aria_core::InputAction::Advance);
    let second = game.step(&advance).unwrap();
    let page = second.view.page.expect("a new backdrop keeps the page");
    assert_eq!(
        (page.tracking, page.at),
        (Some(-20), Some(45)),
        "only tracking changed"
    );

    advance.sequence = 3;
    let third = game.step(&advance).unwrap();
    assert!(
        third.view.page.is_none(),
        "reset returns to the presentation's own page"
    );
}

#[test]
fn page_statement_rejects_values_out_of_range() {
    let output = compile(CompileInput {
        game_id: "jp.example.staging".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source:
                "aria;\nentry start;\nscene start {\n  page from left tracking 900;\n  end;\n}\n"
                    .to_owned(),
        }],
    });
    assert!(output.has_errors());
}

#[test]
fn conditional_options_are_offered_only_while_their_condition_holds() {
    let source = |trust: i64| {
        format!(
            r#"aria;
entry start;
state mut trust: Int = {trust};
scene start {{
  flag "met_mio" = true;
  choice {{
    "海へ行く" => shore;
    "あの話をする" if trust >= 5 && flag("met_mio") => confession;
    "黙っている" => silence;
  }}
}}
scene shore {{ end; }}
scene confession {{ end; }}
scene silence {{ text_speed 0; narrate "黙った。"; await advance; end; }}
"#
        )
    };
    let labels = |trust: i64| -> Vec<String> {
        let mut game = vm(&source(trust));
        game.step(&InputSnapshot::idle(1, 16))
            .unwrap()
            .view
            .choices
            .into_iter()
            .map(|choice| choice.label)
            .collect()
    };
    assert_eq!(labels(2), ["海へ行く", "黙っている"]);
    assert_eq!(labels(5), ["海へ行く", "あの話をする", "黙っている"]);

    // Option ids index what is shown: the second visible option is "黙っている".
    let mut game = vm(&source(2));
    game.step(&InputSnapshot::idle(1, 16)).unwrap();
    let mut pick = InputSnapshot::idle(2, 16);
    pick.intents.push(UiIntent::Activate {
        id: "choice:1".to_owned(),
    });
    game.step(&pick).unwrap();
    let shown = game
        .step(&InputSnapshot::idle(3, 16))
        .unwrap()
        .view
        .dialogue
        .unwrap();
    assert_eq!(shown.full_text, "黙った。");
}

#[test]
fn text_shows_the_current_value_of_named_variables() {
    let mut game = vm(r#"aria;
entry start;
state mut days: Int = 3;
state mut place: String = "松江";
scene start {
  text_speed 0;
  days += 1;
  narrate "{place}を出て、{days}日が経った。{wait:200}";
  await advance;
  end;
}
"#);
    let dialogue = game
        .step(&InputSnapshot::idle(1, 16))
        .unwrap()
        .view
        .dialogue
        .unwrap();
    assert_eq!(dialogue.full_text, "松江を出て、4日が経った。");
}

#[test]
fn unknown_interpolated_names_are_compile_errors() {
    let output = compile(CompileInput {
        game_id: "jp.example.staging".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: "aria;\nentry start;\nscene start { narrate \"{dayz}日\"; end; }\n".to_owned(),
        }],
    });
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("{dayz}"))
    );
}
