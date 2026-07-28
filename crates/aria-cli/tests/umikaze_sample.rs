use std::path::PathBuf;

use aria_cli::project::LoadedProject;
use aria_core::bytecode::{ByteOp, LanguageVersion};
use aria_core::{InputSnapshot, LogicalSize, UiIntent, UiRoute, Vm};

#[test]
fn umikaze_sample_compiles_as_declarative_v32_without_host_opcodes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/umikaze");
    let project = LoadedProject::load(&root).expect("sample manifest should load");
    let output = project
        .compile()
        .expect("sample assets should be inspectable");
    assert!(!output.has_errors(), "{:#?}", output.diagnostics);
    let program = output.program.expect("sample should produce bytecode");
    assert_eq!(program.language_version, LanguageVersion::CURRENT);
    assert_eq!(project.manifest.presentation.frontend, "ui");
    let entry = std::fs::read_to_string(root.join("scripts/main.aria")).unwrap();
    assert!(!entry.contains("ui_theme"));
    assert!(!entry.contains("ui_screen"));
    assert!(entry.contains("use \"scenario/ja-JP/index.aria\";"));
    assert!(!entry.contains("screen setup;"));
    assert!(!entry.contains("scenario/en-US.aria"));
    assert_eq!(project.manifest.runtime.save_namespace, "umikaze-v4");
    assert_eq!(
        project.manifest.runtime.legacy_save_namespaces,
        vec!["umikaze-v3"],
    );
    assert!(
        program
            .instructions
            .iter()
            .any(|instruction| instruction.op == ByteOp::SetLocale)
    );
    assert!(
        program
            .instructions
            .iter()
            .any(|instruction| instruction.op == ByteOp::SetChapterProgress)
    );
}

#[test]
fn umikaze_japanese_scenario_runs_canonical_day_zero_to_ten_chapter_modules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/umikaze");
    let scenario = root.join("scripts/scenario/ja-JP");
    let index = std::fs::read_to_string(scenario.join("index.aria"))
        .expect("Japanese scenario index should be present");

    assert!(!index.contains("canonical.aria"));

    let source_names = [
        "00_init.md",
        "01_start.md",
        "02_day2.md",
        "03_day3.md",
        "04_day4.md",
        "05_day5.md",
        "06_day6.md",
        "07_day7.md",
        "08_day8.md",
        "09_day9.md",
        "10_day10.md",
    ];
    for (chapter, source_name) in source_names.iter().enumerate() {
        let file_name = format!("chapter-{chapter:02}.aria");
        let source = std::fs::read_to_string(scenario.join(&file_name))
            .unwrap_or_else(|_| panic!("missing {file_name}"));
        assert!(
            index.contains(&format!("use \"{file_name}\";")),
            "index should import {file_name}",
        );
        assert!(
            source.contains(&format!("module umikaze.scenario.ja.chapter_{chapter:02};")),
            "{file_name} should retain its own module boundary",
        );
        assert!(
            source.contains(&format!("// Source: {source_name} —")),
            "{file_name} should retain provenance for {source_name}",
        );
        assert!(
            source.contains(&format!("scene novel_chapter_{chapter:02} {{")),
            "{file_name} should expose Day {chapter}",
        );
        assert!(source.contains("screen day_card;"));
        assert!(source.contains(&format!("=> novel_chapter_{chapter:02}_story;")));
        assert!(source.contains(&format!("scene novel_chapter_{chapter:02}_story {{")));
    }
    let chapter_zero = std::fs::read_to_string(scenario.join("chapter-00.aria")).unwrap();
    let chapter_five = std::fs::read_to_string(scenario.join("chapter-05.aria")).unwrap();
    let chapter_ten = std::fs::read_to_string(scenario.join("chapter-10.aria")).unwrap();
    assert!(chapter_zero.contains("screen interlude;"));
    assert!(chapter_five.contains("background asset(\"#05070b\") with fade(2000ms);"));
    assert!(chapter_ten.contains("effect tint \"#05070b\" amount 64 over 520ms;"));
    assert!(!chapter_ten.contains("day10 end"));

    assert!(
        !scenario.join("chapter-11.aria").exists() && !scenario.join("chapter-12.aria").exists(),
        "unfinished DAY 14 and epilogue must not be active Aria sources",
    );
    assert!(
        root.join("docs/drafts/day-14.aria.md").is_file(),
        "the old DAY 14 must survive as a noncompiled draft",
    );
    assert!(root.join("docs/drafts/epilogue.aria.md").is_file());

    let project = LoadedProject::load(&root).unwrap();
    let sources = project.sources().unwrap();
    assert!(sources.iter().all(|source| {
        !source.logical_path.contains("canonical.aria")
            && !source.logical_path.contains("chapter-11.aria")
            && !source.logical_path.contains("chapter-12.aria")
            && !source.logical_path.contains("en-US.aria")
            && !source.logical_path.contains("zh-CN.aria")
            && !source.logical_path.contains("zh-TW.aria")
    }));

    assert!(
        !root.join("scripts/scenario/ja-JP.aria").exists(),
        "the legacy monolithic Japanese scenario must stay removed",
    );
}

#[test]
fn canonical_route_holds_on_day_cards_before_entering_the_source_text() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/umikaze");
    let project = LoadedProject::load(&root).unwrap();
    let program = project.compile().unwrap().program.unwrap();
    let size = LogicalSize {
        width: project.manifest.runtime.logical_width,
        height: project.manifest.runtime.logical_height,
    };

    let mut prologue = Vm::new(program.clone(), size).unwrap();
    let title = prologue.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert_eq!(title.view.route, UiRoute::Title);
    let catalogue = activate(&mut prologue, 2, "choice:0");
    assert_eq!(catalogue.view.route, UiRoute::ChapterSelect);
    assert_eq!(catalogue.view.choices.len(), 11);
    let card = activate(&mut prologue, 3, "choice:0");
    assert_eq!(card.view.route, UiRoute::Custom("day_card".to_owned()));
    assert_eq!(
        card.view.choices[0].label,
        "PROLOGUE\n春から九月\n季節だけが先に進む窓辺で、まだ名もない願いが揺れている。"
    );
    let opening_hold = activate(&mut prologue, 4, "choice:0");
    assert_eq!(opening_hold.view.route, UiRoute::Dialogue);
    let opening = prologue.step(&InputSnapshot::idle(5, 200)).unwrap();
    assert_eq!(
        opening.view.dialogue.unwrap().full_page_text,
        "病室の窓から見える景色は、毎日少しずつ変わっていく。"
    );

    let mut day_ten = Vm::new(program, size).unwrap();
    let _ = day_ten.step(&InputSnapshot::idle(1, 16)).unwrap();
    let catalogue = activate(&mut day_ten, 2, "choice:0");
    assert_eq!(catalogue.view.route, UiRoute::ChapterSelect);
    let card = activate(&mut day_ten, 3, "choice:10");
    assert_eq!(card.view.route, UiRoute::Custom("day_card".to_owned()));
    assert_eq!(
        card.view.choices[0].label,
        "DAY 10\n終点を知らない列車\n灰色の海のそばを、降りる理由のないまま進む。"
    );
    let _ = activate(&mut day_ten, 4, "choice:0");
    let opening = day_ten.step(&InputSnapshot::idle(5, 200)).unwrap();
    assert_eq!(
        opening.view.dialogue.unwrap().full_page_text,
        "硬いベンチで目を覚ますと、待合室に朝の光が差し込んでいた。"
    );
}

#[test]
fn demo_variant_compiles_only_the_opening_arc_and_closes_after_day_four() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/umikaze");
    let project = LoadedProject::load(&root)
        .unwrap()
        .with_runtime_overrides(Some("scripts/main-demo.aria"), Some("umikaze-demo-v1"))
        .unwrap();
    assert_eq!(project.manifest.runtime.save_namespace, "umikaze-demo-v1");
    assert!(project.manifest.runtime.legacy_save_namespaces.is_empty());

    let program = project.compile().unwrap().program.unwrap();
    assert!(program.source_map.iter().all(|location| {
        !matches!(
            location.source.as_str(),
            "scripts/scenario/ja-JP/chapter-05.aria"
                | "scripts/scenario/ja-JP/chapter-06.aria"
                | "scripts/scenario/ja-JP/chapter-07.aria"
                | "scripts/scenario/ja-JP/chapter-08.aria"
                | "scripts/scenario/ja-JP/chapter-09.aria"
                | "scripts/scenario/ja-JP/chapter-10.aria"
        )
    }));
    let size = LogicalSize {
        width: project.manifest.runtime.logical_width,
        height: project.manifest.runtime.logical_height,
    };
    let mut vm = Vm::new(program, size).unwrap();
    let _ = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    let catalogue = activate(&mut vm, 2, "choice:0");
    assert_eq!(catalogue.view.route, UiRoute::ChapterSelect);
    assert_eq!(
        catalogue
            .view
            .choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>(),
        vec!["PROLOGUE", "DAY 1", "DAY 2", "DAY 3", "DAY 4"]
    );

    let card = activate(&mut vm, 3, "choice:4");
    assert_eq!(card.view.route, UiRoute::Custom("day_card".to_owned()));
    let mut output = activate(&mut vm, 4, "choice:0");
    // Timed breaths are story-owned time, not extra reader inputs.  Drive
    // those holds forward in one bounded idle step; otherwise this test would
    // mistake intentional 170ms silences for hundreds of missing advances.
    for sequence in 5..1_500 {
        if output.view.route == UiRoute::DemoEnd {
            break;
        }
        if output.view.route != UiRoute::Custom("interlude".to_owned())
            && output.view.timed_hold_remaining_ms.is_some()
        {
            output = vm.step(&InputSnapshot::idle(sequence, 250)).unwrap();
            continue;
        }
        let action = if output.view.route == UiRoute::Custom("interlude".to_owned()) {
            "interlude.advance"
        } else {
            "dialogue.advance"
        };
        output = activate(&mut vm, sequence, action);
    }
    assert_eq!(output.view.route, UiRoute::DemoEnd);
    assert_eq!(output.view.choices.len(), 2);
    assert_eq!(output.view.choices[0].label, "もう一度読む");
    assert_eq!(output.view.choices[1].label, "タイトルへ戻る");
}

fn activate(vm: &mut Vm, sequence: u64, id: &str) -> aria_core::StepOutput {
    let mut input = InputSnapshot::idle(sequence, 16);
    input.intents.push(UiIntent::Activate { id: id.to_owned() });
    vm.step(&input).unwrap()
}
