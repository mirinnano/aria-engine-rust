use aria_core::protocol::LogicalSize;
use aria_core::{CompileInput, InputSnapshot, SourceUnit, UiIntent, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn compile_source(source: &str) -> aria_core::CompileOutput {
    compile(CompileInput {
        game_id: "jp.example.declarations".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    })
}

const STORY: &str = r#"aria;
entry catalogue;

character mio { name "ミオ"; }

chapter day01 {
  label "DAY 1";
  numeral "I";
  title "西へ";
  date "9月21日・横浜駅";
  synopsis "西へ向かう最初の列車が、朝のホームを離れる。";
  image asset("assets/bg/platform.webp");
  start day01_card;
}

chapter prologue {
  label "PROLOGUE";
  title "病室の窓";
  start prologue_card;
}

scene catalogue {
  screen chapter_select;
  choice {
    chapter prologue;
    chapter day01;
  }
}

scene prologue_card { narrate "冒頭。"; await advance; end; }

scene day01_card {
  screen dialogue;
  choice { chapter day01 => day01_story; }
}

scene day01_story {
  text_speed 0;
  screen dialogue;
  say mio "行こう。";
  await advance;
  end;
}
"#;

#[test]
fn chapter_options_take_label_target_and_metadata_from_the_declaration() {
    let output = compile_source(STORY);
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let mut vm = Vm::new(output.program.unwrap(), SIZE).unwrap();
    let catalogue = vm.step(&InputSnapshot::idle(1, 16)).unwrap();

    let labels: Vec<_> = catalogue
        .view
        .choices
        .iter()
        .map(|choice| choice.label.as_str())
        .collect();
    assert_eq!(labels, ["病室の窓", "西へ"]);
    assert_eq!(catalogue.view.choices[1].chapter.as_deref(), Some("day01"));

    // Declared chapters come in source order with their catalogue fields.
    let day01 = &catalogue.view.chapters[0];
    assert_eq!(day01.id, "day01");
    assert_eq!(day01.label.as_deref(), Some("DAY 1"));
    assert_eq!(day01.numeral.as_deref(), Some("I"));
    assert_eq!(day01.title, "西へ");
    assert_eq!(day01.date.as_deref(), Some("9月21日・横浜駅"));
    assert_eq!(
        day01.description,
        "西へ向かう最初の列車が、朝のホームを離れる。"
    );
    assert_eq!(day01.thumbnail.as_deref(), Some("assets/bg/platform.webp"));
    assert!(!day01.unlocked);
}

#[test]
fn a_declared_character_is_shown_by_name_and_carries_its_id() {
    let output = compile_source(
        r#"aria;
entry start;
character mio { name "ミオ"; }
scene start { text_speed 0; say mio "行こう。"; await advance; end; }
"#,
    );
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let mut vm = Vm::new(output.program.unwrap(), SIZE).unwrap();
    let step = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    let dialogue = step.view.dialogue.unwrap();
    assert_eq!(dialogue.speaker.as_deref(), Some("ミオ"));
    assert_eq!(dialogue.speaker_id.as_deref(), Some("mio"));
}

#[test]
fn undeclared_references_are_reported() {
    let output = compile_source(
        r#"aria;
entry start;
character mio { name "ミオ"; }
chapter lost { title "迷子"; }
scene start {
  say rin "誰？";
  await advance;
  choice { chapter missing; chapter lost; }
}
"#,
    );
    let messages: Vec<_> = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("'rin' is not a declared character")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("undeclared chapter 'missing'")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("chapter 'lost' has no 'start'")),
        "{messages:?}"
    );
}

#[test]
fn choosing_a_chapter_option_enters_its_start_scene() {
    let output = compile_source(STORY);
    let mut vm = Vm::new(output.program.unwrap(), SIZE).unwrap();
    let _ = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    let mut pick = InputSnapshot::idle(2, 16);
    pick.intents.push(UiIntent::Activate {
        id: "choice:0".to_owned(),
    });
    let _ = vm.step(&pick).unwrap();
    let step = vm.step(&InputSnapshot::idle(3, 16)).unwrap();
    assert_eq!(step.view.dialogue.unwrap().full_text, "冒頭。");
}

const MATERIAL: &str = r#"aria;
entry start;

collection title_quotes {
  item world { text "世界は、起こっていることの総体である。"; source "論考 1"; }
  item limits { text "私の言語の限界が、私の世界の限界を意味する。"; source "論考 5.6"; }
}

cg shore_dawn { title "夜明けの浜"; image asset("assets/bg/shore.webp"); }

scene start {
  unlock cg "shore_dawn";
  narrate "海。";
  await advance;
  end;
}
"#;

#[test]
fn collections_and_cg_declarations_reach_the_view_model() {
    let output = compile_source(MATERIAL);
    assert!(!output.has_errors(), "{:#?}", output.diagnostics);
    let mut vm = Vm::new(output.program.unwrap(), SIZE).unwrap();
    let view = vm.step(&InputSnapshot::idle(1, 0)).unwrap().view;
    let quotes = &view.collections["title_quotes"];
    assert_eq!(
        quotes
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["world", "limits"],
        "items keep their source order"
    );
    assert_eq!(quotes[1].fields["source"], "論考 5.6");
    let cg = view
        .gallery
        .iter()
        .find(|item| item.id == "shore_dawn")
        .unwrap();
    assert_eq!(cg.title.as_deref(), Some("夜明けの浜"));
    assert_eq!(cg.image.as_deref(), Some("assets/bg/shore.webp"));
    // Declarations are program data, not save data.
    let saved = serde_json::to_string(&vm.snapshot()).unwrap();
    assert!(!saved.contains("夜明けの浜") && !saved.contains("論考"));
}

#[test]
fn a_collection_spans_modules_but_its_item_ids_stay_unique() {
    let output = compile(CompileInput {
        game_id: "jp.example.declarations".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![
            SourceUnit {
                logical_path: "scripts/main.aria".to_owned(),
                source: r#"aria; use "more.aria"; entry start;
collection notes { item a { text "一"; } }
scene start { end; }"#
                    .to_owned(),
            },
            SourceUnit {
                logical_path: "scripts/more.aria".to_owned(),
                source: r#"aria; module more; collection notes { item b { text "二"; } item a { text "重複"; } }"#
                    .to_owned(),
            },
        ],
    });
    assert!(
        output.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("duplicate item 'a' in collection 'notes'")),
        "{:#?}",
        output.diagnostics
    );
    let duplicate_cg = compile_source(
        r#"aria; entry start; cg x { title "一"; } cg x { title "二"; } scene start { end; }"#,
    );
    assert!(
        duplicate_cg
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("duplicate CG 'x'"))
    );
}
