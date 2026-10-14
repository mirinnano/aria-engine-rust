use aria_core::bytecode::ByteOp;
use aria_core::markup::MarkupKind;
use aria_core::protocol::LogicalSize;
use aria_core::{CompileInput, InputSnapshot, SourceUnit, Vm, compile};

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
