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
