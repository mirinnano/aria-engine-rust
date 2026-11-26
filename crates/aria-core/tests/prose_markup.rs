use aria_core::bytecode::ByteOp;
use aria_core::markup::MarkupKind;
use aria_core::protocol::LogicalSize;
use aria_core::{CompileInput, InputAction, InputSnapshot, SourceUnit, Vm, compile};

const SIZE: LogicalSize = LogicalSize {
    width: 1280,
    height: 720,
};

fn program(source: &str) -> aria_core::CompiledProgram {
    let output = compile(CompileInput {
        game_id: "jp.example.prose".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    });
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    output.program.unwrap()
}

fn ops(program: &aria_core::CompiledProgram) -> Vec<(ByteOp, usize)> {
    program
        .instructions
        .iter()
        .map(|instruction| (instruction.op, instruction.operands.len()))
        .collect()
}

#[test]
fn prose_expands_to_the_hand_written_reading_sequence() {
    let prose = program(
        r#"aria;
entry start;
character mio { name "ミオ"; }
scene start {
  prose breath 220ms {
    "一行目。";
    "続く行、" +;
    "その続き。";
    pause 480ms;
    mio "台詞。";
  }
  end;
}
"#,
    );
    let manual = program(
        r#"aria;
entry start;
character mio { name "ミオ"; }
scene start {
  narrate "一行目。";
  await advance;
  clear dialogue;
  wait breath 220ms;
  narrate "続く行、";
  await advance;
  narrate "その続き。";
  await advance;
  clear dialogue;
  wait breath 480ms;
  say mio "台詞。";
  await advance;
  clear dialogue;
  wait breath 220ms;
  end;
}
"#,
    );
    // Every line matches the hand-written sequence except the one after
    // `+`, which carries the append marker (and an empty speaker-id slot).
    let mut expected = ops(&manual);
    let continued = expected
        .iter()
        .enumerate()
        .filter(|(_, (op, _))| *op == ByteOp::Text)
        .nth(2)
        .map(|(index, _)| index)
        .unwrap();
    expected[continued].1 = 5;
    assert_eq!(ops(&prose), expected);
}

fn press(vm: &mut Vm, sequence: u64) -> aria_core::StepOutput {
    vm.step(&InputSnapshot::pressed(sequence, 16, InputAction::Advance))
        .unwrap()
}

#[test]
fn a_line_after_plus_is_appended_to_the_shown_text() {
    let mut vm = Vm::new(
        program(
            r#"aria;
entry start;
scene start {
  text_speed 0;
  prose {
    "小学校の時に倒れて以来、" +;
    "私の世界は《《この部屋》》だけになった。";
    "次の行。";
  }
  end;
}
"#,
        ),
        SIZE,
    )
    .unwrap();
    let first = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    let first_page = first_page_id(&first.view);
    let dialogue = first.view.dialogue.unwrap();
    assert_eq!(dialogue.full_text, "小学校の時に倒れて以来、");
    assert!(dialogue.awaiting_advance);

    press(&mut vm, 2);
    // As with any line, the appended text is revealed from the next step.
    let joined = vm.step(&InputSnapshot::idle(3, 16)).unwrap();
    let dialogue = joined.view.dialogue.unwrap();
    assert_eq!(dialogue.text, dialogue.full_page_text);
    assert_eq!(
        dialogue.full_text,
        "小学校の時に倒れて以来、私の世界はこの部屋だけになった。"
    );
    assert_eq!(dialogue.page_id, first_page);
    // The emphasis keeps its place after the first line's 12 graphemes.
    assert_eq!(dialogue.markup.len(), 1);
    assert_eq!(dialogue.markup[0].kind, MarkupKind::Emphasis);
    assert!(dialogue.markup[0].start >= 12);
    // History holds the joined page once, not the prefix and the whole.
    assert_eq!(vm.backlog().len(), 1);
    assert!(vm.backlog()[0].text.contains("この部屋"));

    let next = press(&mut vm, 4);
    let next = next.view.dialogue.map(|d| d.full_text).unwrap_or_default();
    // The joined text is cleared before the next line.
    assert!(!next.contains("小学校"), "{next}");
}

fn first_page_id(view: &aria_core::UiViewModel) -> String {
    view.dialogue.as_ref().unwrap().page_id.clone()
}

fn diagnostics(source: &str) -> Vec<String> {
    compile(CompileInput {
        game_id: "jp.example.prose".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: source.to_owned(),
        }],
    })
    .diagnostics
    .into_iter()
    .map(|diagnostic| diagnostic.message)
    .collect()
}

#[test]
fn plus_is_checked_against_what_follows_it() {
    let wrap = |body: &str| {
        format!(
            "aria;\nentry start;\ncharacter mio {{ name \"ミオ\"; }}\nscene start {{ prose {{ {body} }} end; }}\n"
        )
    };
    let speaker = diagnostics(&wrap(r#""地の文、" +; mio "台詞。";"#));
    assert!(
        speaker.iter().any(|m| m.contains("same speaker")),
        "{speaker:?}"
    );
    let pause = diagnostics(&wrap(r#""続く、" +; pause 300ms; "続き。";"#));
    assert!(pause.iter().any(|m| m.contains("'pause'")), "{pause:?}");
    // On the last line, `+` leaves the text shown for what follows `prose`.
    let last = diagnostics(&wrap(r#""残る、" +;"#));
    assert!(last.is_empty(), "{last:?}");
}

#[test]
fn plus_on_the_last_line_leaves_the_text_without_a_breath() {
    let prose = program(
        r#"aria;
entry start;
scene start {
  prose breath 220ms { "残る。" +; }
  clear dialogue;
  end;
}
"#,
    );
    let manual = program(
        r#"aria;
entry start;
scene start {
  narrate "残る。";
  await advance;
  clear dialogue;
  end;
}
"#,
    );
    assert_eq!(ops(&prose), ops(&manual));
}

#[test]
fn ruby_and_emphasis_reach_the_view_as_page_ranges() {
    let mut vm = Vm::new(
        program(
            r#"aria;
entry start;
scene start { text_speed 0; narrate "|宍道湖《しんじこ》は《《広い》》。"; await advance; end; }
"#,
        ),
        SIZE,
    )
    .unwrap();
    let dialogue = vm
        .step(&InputSnapshot::idle(1, 16))
        .unwrap()
        .view
        .dialogue
        .unwrap();
    assert_eq!(dialogue.full_text, "宍道湖は広い。");
    assert_eq!(dialogue.markup.len(), 2);
    assert_eq!((dialogue.markup[0].start, dialogue.markup[0].end), (0, 3));
    assert_eq!(
        dialogue.markup[0].kind,
        MarkupKind::Ruby {
            text: "しんじこ".to_owned()
        }
    );
    assert_eq!((dialogue.markup[1].start, dialogue.markup[1].end), (4, 6));
    assert_eq!(dialogue.markup[1].kind, MarkupKind::Emphasis);
}

#[test]
fn an_inline_wait_holds_the_typewriter_once() {
    let mut vm = Vm::new(
        program(
            r#"aria;
entry start;
scene start { text_speed 10; narrate "あ{wait:500}い"; await advance; end; }
"#,
        ),
        SIZE,
    )
    .unwrap();
    // 10ms reveals "あ"; the following 500ms are the authored pause.
    let shown = |vm: &mut Vm, sequence: u64, delta: u32| {
        vm.step(&InputSnapshot::idle(sequence, delta))
            .unwrap()
            .view
            .dialogue
            .unwrap()
            .text
    };
    // The line is placed on the first step and revealed from the next.
    assert_eq!(shown(&mut vm, 1, 0), "");
    assert_eq!(shown(&mut vm, 2, 10), "あ");
    assert_eq!(shown(&mut vm, 3, 400), "あ");
    assert_eq!(shown(&mut vm, 4, 120), "あい");
}

#[test]
fn malformed_markup_is_a_compile_error() {
    let output = compile(CompileInput {
        game_id: "jp.example.prose".to_owned(),
        entry: "scripts/main.aria".to_owned(),
        sources: vec![SourceUnit {
            logical_path: "scripts/main.aria".to_owned(),
            source: "aria;\nentry start;\nscene start { narrate \"あ《い》\"; end; }\n".to_owned(),
        }],
    });
    assert!(output.has_errors());
}

#[test]
fn the_typewriter_resumes_after_the_previous_line() {
    let mut vm = Vm::new(
        program(
            r#"aria;
entry start;
scene start {
  text_speed 10;
  prose {
    "あい" +;
    "うえ";
  }
  end;
}
"#,
        ),
        SIZE,
    )
    .unwrap();
    let shown = |vm: &mut Vm, sequence: u64, delta: u32| {
        vm.step(&InputSnapshot::idle(sequence, delta))
            .unwrap()
            .view
            .dialogue
            .unwrap()
            .text
    };
    shown(&mut vm, 1, 0);
    assert_eq!(shown(&mut vm, 2, 20), "あい");
    press(&mut vm, 3);
    // The first line stays; only the new line is typed.
    assert_eq!(shown(&mut vm, 4, 0), "あい");
    assert_eq!(shown(&mut vm, 5, 10), "あいう");
}

#[test]
fn clear_dialogue_after_plus_starts_the_next_line_afresh() {
    let prose = program(
        r#"aria;
entry start;
scene start {
  prose {
    "眠りについた。" +;
    clear dialogue;
    wait 300ms;
    "目を覚ます。";
  }
  end;
}
"#,
    );
    let texts: Vec<usize> = prose
        .instructions
        .iter()
        .filter(|instruction| instruction.op == ByteOp::Text)
        .map(|instruction| instruction.operands.len())
        .collect();
    // Neither line carries the append marker.
    assert_eq!(texts, vec![3, 3]);
}
