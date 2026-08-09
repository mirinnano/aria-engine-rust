use std::path::PathBuf;

use aria_cli::project::LoadedProject;
use aria_core::bytecode::{ByteOp, LanguageVersion};
use aria_core::vm::{ExecutionState, VmErrorKind};
use aria_core::{InputSnapshot, LogicalSize, UiIntent, UiRoute, Vm, VmError};

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
    assert_eq!(project.manifest.runtime.save_namespace, "umikaze-v5");
    assert_eq!(
        project.manifest.runtime.legacy_save_namespaces,
        vec!["umikaze-v3", "umikaze-v4"],
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
        assert!(
            !source.contains(&format!(
                "unlock chapter \"canonical_chapter_{chapter:02}\" progress 1;"
            )),
            "{file_name} must not self-unlock at entry",
        );
        assert!(source.contains(&format!(
            "chapter \"canonical_chapter_{chapter:02}\" progress 100;\n  persistent flag \"canonical_chapter_{chapter:02}_completed\" = true;"
        )));
        if chapter < source_names.len() - 1 {
            assert!(source.contains(&format!(
                "unlock chapter \"canonical_chapter_{:02}\" progress 1;",
                chapter + 1
            )));
        } else {
            assert!(
                !source.contains("unlock chapter"),
                "Day 10 completes only itself",
            );
        }
    }
    for chapter in 0..=10 {
        assert!(index.contains(&format!(
            "chapter \"canonical_chapter_{chapter:02}\" progress 0;"
        )));
    }
    assert_eq!(
        index.matches("unlock chapter").count(),
        1,
        "only the prologue is initially unlocked",
    );
    assert!(index.contains("unlock chapter \"canonical_chapter_00\" progress 1;"));
    let chapter_zero = std::fs::read_to_string(scenario.join("chapter-00.aria")).unwrap();
    let chapter_five = std::fs::read_to_string(scenario.join("chapter-05.aria")).unwrap();
    let chapter_ten = std::fs::read_to_string(scenario.join("chapter-10.aria")).unwrap();
    assert!(chapter_zero.contains("screen interlude;"));
    assert!(
        chapter_five
            .contains("play bgm asset(\"assets/audio/bgm/umk.rain.room.ogg\") loop fade 800ms;")
    );
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
    assert!(catalogue.view.choices[0].enabled);
    assert!(catalogue.view.choices[0].unlocked);
    assert!(
        catalogue.view.choices[1..]
            .iter()
            .all(|choice| !choice.enabled && !choice.unlocked)
    );
    assert_eq!(
        catalogue
            .view
            .chapters
            .iter()
            .filter(|chapter| chapter.unlocked)
            .map(|chapter| chapter.id.as_str())
            .collect::<Vec<_>>(),
        vec!["canonical_chapter_00"],
    );
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
        "春から秋　ミオ"
    );

    // This stages the generated Day 0 completion tail at its real compiled
    // `chapter progress 100` instruction. It does not pre-unlock Day 1: the
    // generated tail itself must persist completion and unlock the next day.
    let mut progression = Vm::new(program.clone(), size).unwrap();
    let _ = progression.step(&InputSnapshot::idle(1, 16)).unwrap();
    let initial_catalogue = activate(&mut progression, 2, "choice:0");
    assert!(!initial_catalogue.view.choices[1].enabled);
    run_generated_day_zero_completion_tail(&mut progression, &program, 3);
    // Core's chapter-select surface intentionally does not tick story waits.
    // Keep the real generated tail, but stage its final 480ms return hold on
    // the reading route so that it can finish and re-enter the selector.
    let mut tail_hold = progression.snapshot();
    tail_hold.ui.route = "dialogue".to_owned();
    progression.restore(tail_hold).unwrap();
    let progressed_catalogue = progression.step(&InputSnapshot::idle(1, 480)).unwrap();
    assert_eq!(progressed_catalogue.view.route, UiRoute::ChapterSelect);
    assert!(progressed_catalogue.view.choices[0].enabled);
    assert!(progressed_catalogue.view.choices[1].enabled);
    assert!(progressed_catalogue.view.chapters[1].unlocked);
    assert_eq!(
        progression.snapshot().chapters["canonical_chapter_00"].progress,
        100
    );
    assert!(progression.snapshot().persistent_flags["canonical_chapter_00_completed"]);

    let mut day_ten = Vm::new(program, size).unwrap();
    let _ = day_ten.step(&InputSnapshot::idle(1, 16)).unwrap();
    let catalogue = activate(&mut day_ten, 2, "choice:0");
    assert_eq!(catalogue.view.route, UiRoute::ChapterSelect);
    assert!(!catalogue.view.choices[10].enabled);
    let locked_snapshot = day_ten.snapshot();
    assert!(matches!(
        activate_result(&mut day_ten, 3, "choice:10"),
        Err(VmError::Runtime {
            kind: VmErrorKind::LockedChoice(10),
            ..
        })
    ));
    let still_locked = day_ten.step(&InputSnapshot::idle(4, 16)).unwrap();
    assert_eq!(still_locked.view.route, UiRoute::ChapterSelect);
    assert!(!still_locked.view.choices[10].enabled);

    // Day 10's card/text assertion is content-specific, so stage only this
    // chapter after the ordinary locked-route assertion and restore exactly.
    let mut staged = locked_snapshot.clone();
    stage_chapter_choice_unlocked(&mut staged, "canonical_chapter_10", 10);
    day_ten.restore(staged).unwrap();
    let card = activate(&mut day_ten, 1, "choice:10");
    assert_eq!(card.view.route, UiRoute::Custom("day_card".to_owned()));
    assert_eq!(
        card.view.choices[0].label,
        "DAY 10\n終点を知らない列車\n灰色の海のそばを、降りる理由のないまま進む。"
    );
    let _ = activate(&mut day_ten, 2, "choice:0");
    let opening = day_ten.step(&InputSnapshot::idle(3, 200)).unwrap();
    assert_eq!(
        opening.view.dialogue.unwrap().full_page_text,
        "9月30日　駅の待合室　朝"
    );
    day_ten.restore(locked_snapshot).unwrap();
    let restored_catalogue = day_ten.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert_eq!(restored_catalogue.view.route, UiRoute::ChapterSelect);
    assert!(!restored_catalogue.view.choices[10].enabled);
}

#[test]
fn demo_variant_compiles_only_the_opening_arc_and_closes_after_day_four() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/umikaze");
    let project = LoadedProject::load(&root)
        .unwrap()
        .with_runtime_overrides(Some("scripts/main-demo.aria"), Some("umikaze-demo-v2"))
        .unwrap();
    assert_eq!(project.manifest.runtime.save_namespace, "umikaze-demo-v2");
    // The production demo build uses the CLI's isolated namespace override;
    // that established API intentionally clears full-edition legacy purges.
    // `umikaze-demo-v1` remains the explicit predecessor documented by the
    // game-owned demo build/release configuration.
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
    let full_project = LoadedProject::load(&root).unwrap();
    let full_program = full_project.compile().unwrap().program.unwrap();
    let mut full_vm = Vm::new(
        full_program,
        LogicalSize {
            width: full_project.manifest.runtime.logical_width,
            height: full_project.manifest.runtime.logical_height,
        },
    )
    .unwrap();
    let _ = full_vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    let full_catalogue = activate(&mut full_vm, 2, "choice:0");
    assert_eq!(
        catalogue
            .view
            .choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>(),
        full_catalogue
            .view
            .choices
            .iter()
            .take(5)
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>(),
        "demo catalogue labels must exactly track the generated canonical selector",
    );
    assert_eq!(
        catalogue
            .view
            .choices
            .iter()
            .map(|choice| choice.label.lines().next().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["PROLOGUE", "DAY 1", "DAY 2", "DAY 3", "DAY 4"]
    );
    assert!(catalogue.view.choices[0].enabled);
    assert!(
        catalogue.view.choices[1..]
            .iter()
            .all(|choice| !choice.enabled && !choice.unlocked)
    );
    let locked_snapshot = vm.snapshot();
    assert!(matches!(
        activate_result(&mut vm, 3, "choice:4"),
        Err(VmError::Runtime {
            kind: VmErrorKind::LockedChoice(4),
            ..
        })
    ));
    let still_locked = vm.step(&InputSnapshot::idle(4, 16)).unwrap();
    assert_eq!(still_locked.view.route, UiRoute::ChapterSelect);
    assert!(!still_locked.view.choices[4].enabled);

    // Direct DAY 4 inspection is a content test, not a claim that a fresh
    // demo save exposes it. Restore the locked selector after this staging.
    let mut staged = locked_snapshot.clone();
    stage_chapter_choice_unlocked(&mut staged, "canonical_chapter_04", 4);
    vm.restore(staged).unwrap();
    let card = activate(&mut vm, 1, "choice:4");
    assert_eq!(card.view.route, UiRoute::Custom("day_card".to_owned()));
    let mut output = activate(&mut vm, 2, "choice:0");
    // Timed breaths are story-owned time, not extra reader inputs.  Drive
    // those holds forward in one bounded idle step; otherwise this test would
    // mistake intentional 170ms silences for hundreds of missing advances.
    for sequence in 3..1_500 {
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
        output = activate_result(&mut vm, sequence, action).unwrap_or_else(|error| {
            panic!(
                "demo sequence {sequence}, route {:?}: {error}",
                output.view.route
            )
        });
    }
    assert_eq!(output.view.route, UiRoute::DemoEnd);
    assert_eq!(output.view.choices.len(), 2);
    assert_eq!(output.view.choices[0].label, "もう一度読む");
    assert_eq!(output.view.choices[1].label, "タイトルへ戻る");
    vm.restore(locked_snapshot).unwrap();
    let restored_catalogue = vm.step(&InputSnapshot::idle(1, 16)).unwrap();
    assert_eq!(restored_catalogue.view.route, UiRoute::ChapterSelect);
    assert!(!restored_catalogue.view.choices[4].enabled);
}

fn activate(vm: &mut Vm, sequence: u64, id: &str) -> aria_core::StepOutput {
    activate_result(vm, sequence, id).unwrap()
}

fn activate_result(vm: &mut Vm, sequence: u64, id: &str) -> Result<aria_core::StepOutput, VmError> {
    let mut input = InputSnapshot::idle(sequence, 16);
    input.intents.push(UiIntent::Activate { id: id.to_owned() });
    vm.step(&input)
}

fn run_generated_day_zero_completion_tail(
    vm: &mut Vm,
    program: &aria_core::CompiledProgram,
    sequence: u64,
) {
    let completion_pc = program
        .instructions
        .iter()
        .zip(&program.source_map)
        .position(|(instruction, location)| {
            instruction.op == ByteOp::SetChapterProgress
                && location.source == "scripts/scenario/ja-JP/chapter-00.aria"
        })
        .expect("generated Day 0 completion must compile to SetChapterProgress");
    let mut staged = vm.snapshot();
    staged.pc = u32::try_from(completion_pc).unwrap();
    staged.execution = ExecutionState::Running;
    staged.choice = None;
    vm.restore(staged).unwrap();
    let _ = vm.step(&InputSnapshot::idle(sequence, 16)).unwrap();
}

fn stage_chapter_choice_unlocked(
    snapshot: &mut aria_core::VmSnapshot,
    chapter_id: &str,
    choice_index: usize,
) {
    let chapter = snapshot.chapters.get_mut(chapter_id).unwrap();
    chapter.unlocked = true;
    chapter.progress = 1;
    let choice = snapshot.choice.as_mut().unwrap();
    choice.options[choice_index].enabled = true;
    choice.options[choice_index].unlocked = true;
}
