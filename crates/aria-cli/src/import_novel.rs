//! Faithful Markdown chapter import for prose-first visual-novel projects.
//!
//! The neutral import path does not invent dialogue, backgrounds, or scene
//! direction. It turns each authored, non-empty Markdown line into one Aria
//! reading beat followed by an explicit advance. A project can opt into a
//! narrowly defined non-verbal presentation profile, but the source text
//! remains the authority and the result stays deterministic and reviewable.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aria_core::modern::{Statement, StatementKind, parse as parse_modern};
use atomic_write_file::AtomicWriteFile;
use clap::ValueEnum;
use serde::Serialize;

const BACKGROUND_TONES: [&str; 9] = [
    "#102b38", // tide
    "#284b59", // rooftop
    "#1f3b4d", // platform
    "#3d4655", // photograph
    "#244e5a", // shore
    "#394857", // rain
    "#17253b", // night
    "#315565", // wind
    "#6d6b57", // autumn
];

/// The generated source can either be a neutral prose library or use the
/// deliberately restrained presentation rules owned by the Umikaze sample.
///
/// `Umikaze` is intentionally opt-in: the generic importer must never add
/// project-specific narration or presentation to another game's manuscript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
pub enum NovelPresentation {
    Plain,
    Umikaze,
}

/// The default output is one importable Aria library. Project-owned profiles
/// can also preserve a manuscript's chapter file layout instead of flattening
/// it into one generated source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
pub enum NovelImportLayout {
    Single,
    Chapters,
}

/// Arguments shared by the programmatic and command-line import paths.
/// Keeping them as data makes a generated scenario reproducible in tests and
/// lets a project explicitly constrain the source files it accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NovelImportOptions {
    pub chapter_select: String,
    pub locale: String,
    /// Exact Markdown file names, in source-directory order. An empty list
    /// imports every Markdown chapter in the source directory.
    pub include: Vec<String>,
    pub presentation: NovelPresentation,
    pub layout: NovelImportLayout,
}

impl NovelImportOptions {
    pub fn plain(chapter_select: impl Into<String>, locale: impl Into<String>) -> Self {
        Self {
            chapter_select: chapter_select.into(),
            locale: locale.into(),
            include: Vec::new(),
            presentation: NovelPresentation::Plain,
            layout: NovelImportLayout::Single,
        }
    }
}

// An attribution is metadata only when the author used an explicit character
// label. Keeping this finite list intentionally conservative prevents prose
// such as '小さく「…」' or '俺は「…」' from losing its authored prefix.
const EXPLICIT_SPEAKERS: [&str; 8] = [
    "俺",
    "ミオ",
    "老婆",
    "フロント",
    "駅員",
    "親父",
    "管理人",
    "店員",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NovelImportReport {
    pub source_directory: String,
    pub output: String,
    pub presentation: NovelPresentation,
    pub layout: NovelImportLayout,
    pub chapters: Vec<NovelChapterReport>,
    /// Player-visible authored text, including authored scene headings.
    pub reading_beats: usize,
    pub structural_breaks: usize,
    /// Authored control directives translated into non-verbal stage work.
    pub stage_directions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NovelChapterReport {
    pub source: String,
    pub scene: String,
    pub label: String,
    pub reading_beats: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NovelChapter {
    source_name: String,
    scene: String,
    chapter_id: String,
    label: String,
    beats: Vec<NovelBeat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NovelBeat {
    Reading {
        speaker: Option<String>,
        text: String,
    },
    Heading {
        text: String,
    },
    StructuralBreak,
    Direction(NovelDirection),
}

/// One player-visible source beat. The verifier compares this semantic form,
/// not escaped Aria source bytes, so a change in text, speaker, order, or
/// count cannot hide behind formatting differences.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerText {
    speaker: Option<String>,
    text: String,
}

/// Legacy authored Markdown occasionally contains a compact staging cue. It
/// is not prose, so it must not appear in the reader as a literal command.
/// The Umikaze presentation maps these cues to the equivalent quiet stage
/// transition while the neutral importer preserves their timing only.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NovelDirection {
    FadeOut { duration_ms: u32 },
    StopBgm,
    Wait { duration_ms: u32 },
    FadeIn,
}

/// Executes the command-line form of the importer.
pub fn command(
    source: &Path,
    out: &Path,
    options: NovelImportOptions,
    verify_only: bool,
) -> Result<u8> {
    let report = if verify_only {
        verify_novel_output(source, out, options)?
    } else {
        import_novel_with_options(source, out, options)?
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

/// Imports a canonical Markdown source directory into a standalone Aria
/// library source. The generated module is intentionally source-only: callers
/// import it from their own entry module and retain ownership of title/setup.
pub fn import_novel(
    source: &Path,
    out: &Path,
    chapter_select: &str,
    locale: &str,
) -> Result<NovelImportReport> {
    import_novel_with_options(
        source,
        out,
        NovelImportOptions::plain(chapter_select, locale),
    )
}

/// Imports authored Markdown with explicitly selected source files and
/// presentation rules. The generated file remains standalone: it embeds the
/// prose, never reads the Markdown at runtime, and can therefore ship in a
/// regular game package.
pub fn import_novel_with_options(
    source: &Path,
    out: &Path,
    options: NovelImportOptions,
) -> Result<NovelImportReport> {
    let (source_directory, chapters) = load_novel_chapters(source, &options)?;

    match options.layout {
        NovelImportLayout::Single => {
            let generated = render_module(
                &chapters,
                &options.chapter_select,
                &options.locale,
                options.presentation,
            )?;
            verify_generated_source(out, &generated)?;
            write_atomic(out, generated.as_bytes())?;
        }
        NovelImportLayout::Chapters => {
            if options.presentation != NovelPresentation::Umikaze {
                bail!("the chapter-file layout is currently owned by --presentation umikaze");
            }
            let generated =
                render_umikaze_chapter_layout(&chapters, &options.chapter_select, &options.locale)?;
            write_chapter_layout(out, &generated)?;
        }
    }

    verify_existing_output(out, &chapters, &options)?;
    Ok(novel_import_report(
        &source_directory,
        out,
        &options,
        &chapters,
    ))
}

/// Verifies a previously generated story without writing to it.
///
/// This is deliberately stronger than regenerating and comparing bytes: the
/// Markdown player-text sequence is compared with the parsed Aria scenes, so
/// a manual edit cannot add, drop, reorder, or change a visible beat without
/// being reported.
pub fn verify_novel_output(
    source: &Path,
    out: &Path,
    options: NovelImportOptions,
) -> Result<NovelImportReport> {
    let (source_directory, chapters) = load_novel_chapters(source, &options)?;
    verify_existing_output(out, &chapters, &options)?;
    Ok(novel_import_report(
        &source_directory,
        out,
        &options,
        &chapters,
    ))
}

fn load_novel_chapters(
    source: &Path,
    options: &NovelImportOptions,
) -> Result<(PathBuf, Vec<NovelChapter>)> {
    if !is_aria_identifier(&options.chapter_select) {
        bail!(
            "chapter selector scene must be an Aria identifier: '{}'",
            options.chapter_select
        );
    }
    if options.locale.trim().is_empty() {
        bail!("locale must not be empty");
    }

    let source_directory = source.canonicalize().with_context(|| {
        format!(
            "cannot resolve Markdown source directory {}",
            source.display()
        )
    })?;
    if !source_directory.is_dir() {
        bail!(
            "Markdown source is not a directory: {}",
            source_directory.display()
        );
    }

    let chapters = discover_chapters(&source_directory, &options.include)?;
    if chapters.is_empty() {
        bail!(
            "no Markdown chapter files found in {}",
            source_directory.display()
        );
    }

    Ok((source_directory, chapters))
}

fn novel_import_report(
    source_directory: &Path,
    out: &Path,
    options: &NovelImportOptions,
    chapters: &[NovelChapter],
) -> NovelImportReport {
    let reading_beats = chapters
        .iter()
        .map(|chapter| {
            chapter
                .beats
                .iter()
                .filter(|beat| is_player_text(beat))
                .count()
        })
        .sum();
    let structural_breaks = chapters
        .iter()
        .flat_map(|chapter| &chapter.beats)
        .filter(|beat| matches!(beat, NovelBeat::StructuralBreak))
        .count();
    let stage_directions = chapters
        .iter()
        .flat_map(|chapter| &chapter.beats)
        .filter(|beat| matches!(beat, NovelBeat::Direction(_)))
        .count();

    NovelImportReport {
        source_directory: source_directory.display().to_string(),
        output: out.display().to_string(),
        presentation: options.presentation,
        layout: options.layout,
        chapters: chapters
            .iter()
            .map(|chapter| NovelChapterReport {
                source: chapter.source_name.clone(),
                scene: chapter.scene.clone(),
                label: chapter.label.clone(),
                reading_beats: chapter
                    .beats
                    .iter()
                    .filter(|beat| is_player_text(beat))
                    .count(),
            })
            .collect(),
        reading_beats,
        structural_breaks,
        stage_directions,
    }
}

fn discover_chapters(source_directory: &Path, include: &[String]) -> Result<Vec<NovelChapter>> {
    let requested = include
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    let mut paths = fs::read_dir(source_directory)
        .with_context(|| format!("cannot list {}", source_directory.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            && (requested.is_empty()
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| requested.contains(name)))
    });
    paths.sort_by(|left, right| {
        left.file_name()
            .unwrap_or_default()
            .cmp(right.file_name().unwrap_or_default())
    });

    if !requested.is_empty() {
        let found = paths
            .iter()
            .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
            .collect::<BTreeSet<_>>();
        let missing = requested
            .iter()
            .filter(|name| !found.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!(
                "requested Markdown chapter file(s) were not found in {}: {}",
                source_directory.display(),
                missing.join(", ")
            );
        }
    }

    paths
        .into_iter()
        .enumerate()
        .map(|(index, source_path)| parse_chapter(index, source_path))
        .collect()
}

fn parse_chapter(index: usize, source_path: PathBuf) -> Result<NovelChapter> {
    let source = fs::read_to_string(&source_path)
        .with_context(|| format!("cannot read authored chapter {}", source_path.display()))?;
    let source_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Markdown chapter filename must be valid UTF-8")?
        .to_owned();
    let stem = source_path
        .file_stem()
        .and_then(|name| name.to_str())
        .context("Markdown chapter stem must be valid UTF-8")?;
    let beats = parse_beats(&source);
    if !beats
        .iter()
        .any(|beat| matches!(beat, NovelBeat::Reading { .. }))
    {
        bail!(
            "authored chapter has no readable prose: {}",
            source_path.display()
        );
    }

    Ok(NovelChapter {
        source_name,
        scene: format!("novel_chapter_{index:02}"),
        chapter_id: format!("canonical_chapter_{index:02}"),
        label: chapter_label(stem),
        beats,
    })
}

fn parse_beats(source: &str) -> Vec<NovelBeat> {
    source
        .lines()
        .filter_map(|source_line| {
            let line = source_line.trim_end_matches('\r');
            let trimmed = line.trim();
            if trimmed.is_empty() || is_day_end_marker(trimmed) {
                return None;
            }
            if trimmed.starts_with(';') {
                return None;
            }
            if let Some(direction) = parse_direction(trimmed) {
                return Some(NovelBeat::Direction(direction));
            }
            if trimmed == "* * *" {
                return Some(NovelBeat::StructuralBreak);
            }
            // `# side2` in the canonical manuscript is a source-side POV
            // divider, not player-facing prose. Treat all remaining Markdown
            // headings as a silent structural turn; authored date headings
            // use `**...**` and remain visible below.
            if trimmed.starts_with("# ") {
                return Some(NovelBeat::StructuralBreak);
            }
            if let Some(text) = strip_scene_heading(line) {
                return Some(NovelBeat::Heading { text });
            }

            let (speaker, text) = split_attributed_dialogue(line).map_or_else(
                || (None, line.to_owned()),
                |(speaker, text)| (Some(speaker), text),
            );
            Some(NovelBeat::Reading { speaker, text })
        })
        .collect()
}

fn parse_direction(line: &str) -> Option<NovelDirection> {
    let command = line.trim();
    if command.eq_ignore_ascii_case("stopbgm") {
        return Some(NovelDirection::StopBgm);
    }
    if command.eq_ignore_ascii_case("fadein") {
        return Some(NovelDirection::FadeIn);
    }
    if let Some(seconds) = command.strip_prefix("fadeout ") {
        let seconds = seconds.trim().parse::<u32>().ok()?;
        return seconds
            .checked_mul(1_000)
            .map(|duration_ms| NovelDirection::FadeOut { duration_ms });
    }
    if let Some(milliseconds) = command.strip_prefix("wait ") {
        let duration_ms = milliseconds.trim().parse::<u32>().ok()?;
        return Some(NovelDirection::Wait { duration_ms });
    }
    None
}

fn is_player_text(beat: &NovelBeat) -> bool {
    matches!(beat, NovelBeat::Reading { .. } | NovelBeat::Heading { .. })
}

fn is_day_end_marker(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    (line.starts_with("# ") && lower.ends_with(" end"))
        || (line.starts_with(';') && lower.contains(" end"))
}

fn strip_scene_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix("**")?.strip_suffix("**")?.trim();
    (!inner.is_empty()).then(|| inner.to_owned())
}

/// Recognizes only the deliberately compact, explicit character-label form.
/// Lines starting directly with a quote intentionally stay unattributed
/// because the canonical prose uses context-dependent speakers throughout
/// most chapters.
fn split_attributed_dialogue(line: &str) -> Option<(String, String)> {
    let (speaker, spoken) = line.split_once('「')?;
    if speaker.is_empty() || !EXPLICIT_SPEAKERS.contains(&speaker) || !spoken.ends_with('」') {
        return None;
    }
    Some((speaker.to_owned(), format!("「{spoken}")))
}

fn chapter_label(stem: &str) -> String {
    match stem {
        "00_init" => "序章".to_owned(),
        "ex" | "epilogue" => "後日談".to_owned(),
        _ => stem
            .split_once('_')
            .and_then(|(day, _)| day.parse::<u16>().ok())
            .map_or_else(|| stem.replace('_', " "), |day| format!("DAY {day}")),
    }
}

struct UmikazeChapterStyle {
    source_name: &'static str,
    day: &'static str,
    date: &'static str,
    synopsis: &'static str,
    background: &'static str,
}

// These are navigation metadata rather than new story text. Each line names
// only a place, weather, or motion already present in that chapter; the
// actual prose always comes directly from the canonical Markdown below.
const UMIKAZE_CHAPTER_STYLES: [UmikazeChapterStyle; 11] = [
    UmikazeChapterStyle {
        source_name: "00_init.md",
        day: "PROLOGUE",
        date: "春から九月",
        synopsis: "季節だけが先に進む窓辺で、まだ名もない願いが揺れている。",
        background: "#3d4655",
    },
    UmikazeChapterStyle {
        source_name: "01_start.md",
        day: "DAY 1",
        date: "9月21日・横浜駅",
        synopsis: "西へ向かう最初の列車が、朝のホームを離れる。",
        background: "#284b59",
    },
    UmikazeChapterStyle {
        source_name: "02_day2.md",
        day: "DAY 2",
        date: "9月22日・三ノ宮",
        synopsis: "雨の気配が近づく街で、二人は次の行き先を探している。",
        background: "#1f3b4d",
    },
    UmikazeChapterStyle {
        source_name: "03_day3.md",
        day: "DAY 3",
        date: "9月23日・岡山",
        synopsis: "遠ざかる景色の先で、言葉にできないものと向き合う。",
        background: "#3d4655",
    },
    UmikazeChapterStyle {
        source_name: "04_day4.md",
        day: "DAY 4",
        date: "9月24日・松江",
        synopsis: "残そうとする音が、静かな海辺へ続く道を指している。",
        background: "#244e5a",
    },
    UmikazeChapterStyle {
        source_name: "05_day5.md",
        day: "DAY 5",
        date: "9月25日・益田",
        synopsis: "強い雨が、進む理由を足止めする。",
        background: "#394857",
    },
    UmikazeChapterStyle {
        source_name: "06_day6.md",
        day: "DAY 6",
        date: "晴れた移動の途中",
        synopsis: "夜の駅を越え、海の気配へ向かう。",
        background: "#6d6b57",
    },
    UmikazeChapterStyle {
        source_name: "07_day7.md",
        day: "DAY 7",
        date: "始発前の待合室",
        synopsis: "海を渡るあいだ、記録の外側が近づいてくる。",
        background: "#315565",
    },
    UmikazeChapterStyle {
        source_name: "08_day8.md",
        day: "DAY 8",
        date: "山あいの居間",
        synopsis: "遠い場所の映像が、静かな朝を占めていく。",
        background: "#6d6b57",
    },
    UmikazeChapterStyle {
        source_name: "09_day9.md",
        day: "DAY 9",
        date: "北へ向かう列車",
        synopsis: "足元の地図を離れ、線路だけが先へ続いている。",
        background: "#102b38",
    },
    UmikazeChapterStyle {
        source_name: "10_day10.md",
        day: "DAY 10",
        date: "終点を知らない列車",
        synopsis: "灰色の海のそばを、降りる理由のないまま進む。",
        background: "#17253b",
    },
];

fn render_module(
    chapters: &[NovelChapter],
    chapter_select: &str,
    locale: &str,
    presentation: NovelPresentation,
) -> Result<String> {
    match presentation {
        NovelPresentation::Plain => Ok(render_plain_module(chapters, chapter_select, locale)),
        NovelPresentation::Umikaze => render_umikaze_module(chapters, chapter_select, locale),
    }
}

fn render_plain_module(chapters: &[NovelChapter], chapter_select: &str, locale: &str) -> String {
    let mut output = String::new();
    output.push_str("// Generated by aria import-novel. Do not edit this file by hand.\n");
    output.push_str("// The Markdown source remains the canonical prose authority.\n");
    output.push_str("aria;\n");
    output.push_str("module novel.imported;\n\n");

    output.push_str(&format!("scene {chapter_select} {{\n"));
    output.push_str("  screen chapter_select;\n");
    output.push_str("  background asset(\"#102b38\") with wipe(260ms);\n");
    output.push_str("  choice {\n");
    for chapter in chapters {
        output.push_str(&format!(
            "    \"{}\" => {};\n",
            escape_string(&chapter.label),
            chapter.scene
        ));
    }
    output.push_str("  }\n");
    output.push_str("}\n\n");

    for (index, chapter) in chapters.iter().enumerate() {
        output.push_str(&format!(
            "// Source: {}\nscene {} {{\n",
            chapter.source_name, chapter.scene
        ));
        output.push_str(&format!("  locale \"{}\";\n", escape_string(locale)));
        output.push_str(&format!(
            "  persistent flag \"{}_seen\" = true;\n",
            chapter.chapter_id
        ));
        output.push_str(&format!(
            "  unlock chapter \"{}\" progress 1;\n",
            chapter.chapter_id
        ));
        output.push_str("  screen dialogue;\n");
        output.push_str(&format!(
            "  background asset(\"{}\") with fade(260ms);\n",
            BACKGROUND_TONES[index % BACKGROUND_TONES.len()]
        ));

        for beat in &chapter.beats {
            match beat {
                NovelBeat::Reading { speaker, text } => {
                    render_text_beat(&mut output, speaker.as_deref(), text);
                }
                NovelBeat::Heading { text } => {
                    render_text_beat(&mut output, None, text);
                }
                NovelBeat::StructuralBreak => {
                    output.push_str("  clear dialogue;\n");
                    output.push_str("  wait 550ms;\n");
                }
                NovelBeat::Direction(direction) => render_plain_direction(&mut output, direction),
            }
        }

        output.push_str(&format!(
            "  chapter \"{}\" progress 100;\n",
            chapter.chapter_id
        ));
        output.push_str("  clear dialogue;\n");
        output.push_str(&format!("  jump {chapter_select};\n"));
        output.push_str("}\n\n");
    }
    output
}

fn render_umikaze_module(
    chapters: &[NovelChapter],
    chapter_select: &str,
    locale: &str,
) -> Result<String> {
    let styles = chapters
        .iter()
        .map(umikaze_style_for)
        .collect::<Result<Vec<_>>>()?;
    if styles.len() != UMIKAZE_CHAPTER_STYLES.len() {
        bail!(
            "the Umikaze presentation requires all {} canonical Day 0–10 sources; received {}",
            UMIKAZE_CHAPTER_STYLES.len(),
            styles.len()
        );
    }

    let mut output = String::new();
    output.push_str(
        "// Generated by aria import-novel --presentation umikaze. Do not edit by hand.\n",
    );
    output.push_str("// Canonical prose is embedded from the selected Markdown source files.\n");
    output.push_str(
        "// Stage work is deliberately non-verbal: day cards, headings, fades, and held silence.\n",
    );
    output.push_str("aria;\n");
    output.push_str("module umikaze.scenario.ja.canonical;\n\n");

    output.push_str(&format!("scene {chapter_select} {{\n"));
    output.push_str("  screen chapter_select;\n");
    output.push_str("  background asset(\"#102b38\") with wipe(260ms);\n");
    output.push_str("  choice {\n");
    for (chapter, style) in chapters.iter().zip(&styles) {
        output.push_str(&format!(
            "    \"{}\" => {};\n",
            escape_string(style.day),
            chapter.scene
        ));
    }
    output.push_str("  }\n");
    output.push_str("}\n\n");

    for (chapter, style) in chapters.iter().zip(styles) {
        render_umikaze_chapter_content(&mut output, chapter, style, chapter_select, locale);
    }

    Ok(output)
}

fn render_umikaze_chapter_content(
    output: &mut String,
    chapter: &NovelChapter,
    style: &UmikazeChapterStyle,
    chapter_select: &str,
    locale: &str,
) {
    let story_scene = format!("{}_story", chapter.scene);
    output.push_str(&format!(
        "// Source: {} — {} player-visible beats\n",
        chapter.source_name,
        chapter
            .beats
            .iter()
            .filter(|beat| is_player_text(beat))
            .count()
    ));
    output.push_str(&format!("scene {} {{\n", chapter.scene));
    output.push_str(&format!("  locale \"{}\";\n", escape_string(locale)));
    output.push_str(&format!(
        "  persistent flag \"{}_seen\" = true;\n",
        chapter.chapter_id
    ));
    output.push_str(&format!(
        "  unlock chapter \"{}\" progress 1;\n",
        chapter.chapter_id
    ));
    output.push_str(&format!(
        "  background asset(\"{}\") with fade(260ms);\n",
        style.background
    ));
    output.push_str("  screen day_card;\n");
    output.push_str("  choice {\n");
    output.push_str(&format!(
        "    \"{}\\n{}\\n{}\" => {};\n",
        escape_string(style.day),
        escape_string(style.date),
        escape_string(style.synopsis),
        story_scene
    ));
    output.push_str("  }\n");
    output.push_str("}\n\n");

    output.push_str(&format!("scene {story_scene} {{\n"));
    output.push_str("  screen dialogue;\n");
    output.push_str(&format!(
        "  background asset(\"{}\") with fade(360ms);\n",
        style.background
    ));
    output.push_str("  wait 180ms;\n");

    for beat in &chapter.beats {
        render_umikaze_beat(output, chapter, style, beat);
    }

    output.push_str(&format!(
        "  chapter \"{}\" progress 100;\n",
        chapter.chapter_id
    ));
    output.push_str("  clear dialogue;\n");
    output.push_str("  wait 180ms;\n");
    output.push_str(&format!("  jump {chapter_select};\n"));
    output.push_str("}\n\n");
}

#[derive(Debug)]
struct GeneratedChapterLayout {
    index: String,
    chapters: Vec<GeneratedChapterFile>,
}

#[derive(Debug)]
struct GeneratedChapterFile {
    file_name: String,
    source: String,
}

fn render_umikaze_chapter_layout(
    chapters: &[NovelChapter],
    chapter_select: &str,
    locale: &str,
) -> Result<GeneratedChapterLayout> {
    let styles = chapters
        .iter()
        .map(umikaze_style_for)
        .collect::<Result<Vec<_>>>()?;
    if styles.len() != UMIKAZE_CHAPTER_STYLES.len() {
        bail!(
            "the Umikaze chapter-file layout requires all {} canonical Day 0–10 sources; received {}",
            UMIKAZE_CHAPTER_STYLES.len(),
            styles.len()
        );
    }

    let mut index = String::new();
    index.push_str("aria;\n");
    index.push_str("module umikaze.scenario.ja;\n\n");
    index.push_str("// Generated from the canonical Day 0–10 Markdown source.\n");
    index.push_str("// Each chapter remains its own reviewable Aria module.\n");
    for chapter in chapters {
        index.push_str(&format!("use \"{}\";\n", chapter_file_name(chapter)));
    }
    index.push('\n');
    index.push_str(&format!("scene {chapter_select} {{\n"));
    index.push_str("  screen chapter_select;\n");
    index.push_str("  background asset(\"#102b38\") with wipe(260ms);\n");
    index.push_str("  choice {\n");
    for (chapter, style) in chapters.iter().zip(&styles) {
        index.push_str(&format!(
            "    \"{}\" => {};\n",
            escape_string(style.day),
            chapter.scene
        ));
    }
    index.push_str("  }\n");
    index.push_str("}\n");

    let mut rendered_chapters = Vec::with_capacity(chapters.len());
    for (chapter, style) in chapters.iter().zip(styles) {
        let mut source = String::new();
        source.push_str(
            "// Generated by aria import-novel --presentation umikaze --layout chapters.\n",
        );
        source.push_str("// Canonical prose is embedded from the selected Markdown source file.\n");
        source.push_str("aria;\n");
        source.push_str(&format!(
            "module umikaze.scenario.ja.chapter_{};\n\n",
            chapter_scene_suffix(chapter)
        ));
        render_umikaze_chapter_content(&mut source, chapter, style, chapter_select, locale);
        rendered_chapters.push(GeneratedChapterFile {
            file_name: chapter_file_name(chapter),
            source,
        });
    }

    Ok(GeneratedChapterLayout {
        index,
        chapters: rendered_chapters,
    })
}

fn chapter_scene_suffix(chapter: &NovelChapter) -> &str {
    chapter
        .scene
        .rsplit_once('_')
        .map_or("chapter", |(_, suffix)| suffix)
}

fn chapter_file_name(chapter: &NovelChapter) -> String {
    format!("chapter-{}.aria", chapter_scene_suffix(chapter))
}

fn verify_generated_source(path: &Path, generated: &str) -> Result<()> {
    let parsed = parse_modern(path.to_string_lossy(), generated);
    if parsed.has_errors() {
        let diagnostics = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        bail!("generated Aria source did not parse: {diagnostics}");
    }
    Ok(())
}

fn write_chapter_layout(out: &Path, generated: &GeneratedChapterLayout) -> Result<()> {
    if out.exists() && !out.is_dir() {
        bail!(
            "chapter-file output must be a directory, not a file: {}",
            out.display()
        );
    }
    fs::create_dir_all(out).with_context(|| format!("cannot create {}", out.display()))?;

    let index_path = out.join("index.aria");
    verify_generated_source(&index_path, &generated.index)?;
    write_atomic(&index_path, generated.index.as_bytes())?;
    for chapter in &generated.chapters {
        let path = out.join(&chapter.file_name);
        verify_generated_source(&path, &chapter.source)?;
        write_atomic(&path, chapter.source.as_bytes())?;
    }
    Ok(())
}

fn verify_existing_output(
    out: &Path,
    chapters: &[NovelChapter],
    options: &NovelImportOptions,
) -> Result<()> {
    match options.layout {
        NovelImportLayout::Single => {
            for chapter in chapters {
                verify_chapter_prose(out, chapter, options)?;
            }
        }
        NovelImportLayout::Chapters => {
            if options.presentation != NovelPresentation::Umikaze {
                bail!("the chapter-file layout is currently owned by --presentation umikaze");
            }
            verify_chapter_index(out, chapters, options)?;
            for chapter in chapters {
                verify_chapter_prose(&out.join(chapter_file_name(chapter)), chapter, options)?;
            }
        }
    }
    Ok(())
}

fn verify_chapter_index(
    out: &Path,
    chapters: &[NovelChapter],
    options: &NovelImportOptions,
) -> Result<()> {
    if !out.is_dir() {
        bail!("chapter-file output must be a directory: {}", out.display());
    }
    let index_path = out.join("index.aria");
    let module = parse_generated_module(&index_path)?;
    let actual_imports = module
        .imports
        .iter()
        .map(|import| import.path.clone())
        .collect::<Vec<_>>();
    let expected_imports = chapters.iter().map(chapter_file_name).collect::<Vec<_>>();
    if actual_imports != expected_imports {
        bail!(
            "chapter index import mismatch in {}: expected {:?}, found {:?}",
            index_path.display(),
            expected_imports,
            actual_imports
        );
    }

    let selector = module
        .scenes
        .iter()
        .find(|scene| scene.name == options.chapter_select)
        .with_context(|| {
            format!(
                "chapter index {} does not define scene '{}'",
                index_path.display(),
                options.chapter_select
            )
        })?;
    let actual_targets = selector
        .body
        .iter()
        .find_map(|statement| match &statement.kind {
            StatementKind::Choice { options } => Some(
                options
                    .iter()
                    .map(|option| option.scene.clone())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .context("chapter selector has no choice statement")?;
    let expected_targets = chapters
        .iter()
        .map(|chapter| chapter.scene.clone())
        .collect::<Vec<_>>();
    if actual_targets != expected_targets {
        bail!(
            "chapter selector target mismatch in {}: expected {:?}, found {:?}",
            index_path.display(),
            expected_targets,
            actual_targets
        );
    }
    Ok(())
}

fn verify_chapter_prose(
    path: &Path,
    chapter: &NovelChapter,
    options: &NovelImportOptions,
) -> Result<()> {
    let scene_name = match options.presentation {
        NovelPresentation::Plain => chapter.scene.clone(),
        NovelPresentation::Umikaze => format!("{}_story", chapter.scene),
    };
    let module = parse_generated_module(path)?;
    let scene = module
        .scenes
        .iter()
        .find(|scene| scene.name == scene_name)
        .with_context(|| {
            format!(
                "{} does not define story scene '{}'",
                path.display(),
                scene_name
            )
        })?;
    let expected = expected_player_texts(chapter);
    let actual = actual_player_texts(&scene.body);
    if expected != actual {
        let first_difference = expected
            .iter()
            .zip(&actual)
            .position(|(expected, actual)| expected != actual)
            .unwrap_or_else(|| expected.len().min(actual.len()));
        bail!(
            "player-text mismatch in {} (source {}), beat {}: expected {} beat(s), found {}; expected {:?}, found {:?}",
            path.display(),
            chapter.source_name,
            first_difference + 1,
            expected.len(),
            actual.len(),
            expected.get(first_difference),
            actual.get(first_difference),
        );
    }
    verify_umikaze_chapter_presentation(path, chapter, options, &module)?;
    Ok(())
}

fn parse_generated_module(path: &Path) -> Result<aria_core::modern::ModernModule> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("cannot read generated Aria source {}", path.display()))?;
    let parsed = parse_modern(path.to_string_lossy(), source);
    if parsed.has_errors() {
        let diagnostics = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        bail!(
            "generated Aria source did not parse ({}): {diagnostics}",
            path.display()
        );
    }
    parsed
        .module
        .with_context(|| format!("generated Aria source has no module: {}", path.display()))
}

fn expected_player_texts(chapter: &NovelChapter) -> Vec<PlayerText> {
    chapter
        .beats
        .iter()
        .filter_map(|beat| match beat {
            NovelBeat::Reading { speaker, text } => Some(PlayerText {
                speaker: speaker.clone(),
                text: text.clone(),
            }),
            NovelBeat::Heading { text } => Some(PlayerText {
                speaker: None,
                text: text.clone(),
            }),
            NovelBeat::StructuralBreak | NovelBeat::Direction(_) => None,
        })
        .collect()
}

fn actual_player_texts(statements: &[Statement]) -> Vec<PlayerText> {
    let mut texts = Vec::new();
    collect_player_texts(statements, &mut texts);
    texts
}

fn collect_player_texts(statements: &[Statement], texts: &mut Vec<PlayerText>) {
    for statement in statements {
        match &statement.kind {
            StatementKind::Say { speaker, text } => texts.push(PlayerText {
                speaker: speaker.clone(),
                text: text.clone(),
            }),
            StatementKind::Narrate { text } => texts.push(PlayerText {
                speaker: None,
                text: text.clone(),
            }),
            StatementKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_player_texts(then_branch, texts);
                collect_player_texts(else_branch, texts);
            }
            StatementKind::While { body, .. } | StatementKind::Borrow { body, .. } => {
                collect_player_texts(body, texts);
            }
            _ => {}
        }
    }
}

fn verify_umikaze_chapter_presentation(
    path: &Path,
    chapter: &NovelChapter,
    options: &NovelImportOptions,
    module: &aria_core::modern::ModernModule,
) -> Result<()> {
    if options.presentation != NovelPresentation::Umikaze {
        return Ok(());
    }
    let entry_scene = module
        .scenes
        .iter()
        .find(|scene| scene.name == chapter.scene)
        .with_context(|| {
            format!(
                "{} does not define chapter-card scene '{}'",
                path.display(),
                chapter.scene
            )
        })?;
    let has_day_card = entry_scene.body.iter().any(|statement| {
        matches!(
            &statement.kind,
            StatementKind::OpenScreen { screen } if screen == "day_card"
        )
    });
    if !has_day_card {
        bail!(
            "{} has no day-card hold for source {}",
            path.display(),
            chapter.source_name
        );
    }
    let expected_story = format!("{}_story", chapter.scene);
    let card_targets_story = entry_scene.body.iter().any(|statement| {
        matches!(
            &statement.kind,
            StatementKind::Choice { options }
                if options.len() == 1 && options[0].scene == expected_story
        )
    });
    if !card_targets_story {
        bail!(
            "{} day-card does not enter '{}'",
            path.display(),
            expected_story
        );
    }

    let story = module
        .scenes
        .iter()
        .find(|scene| scene.name == expected_story)
        .expect("story scene was verified before presentation");
    let expected_interludes = chapter
        .beats
        .iter()
        .filter(|beat| matches!(beat, NovelBeat::Heading { .. }))
        .count();
    let actual_interludes = story
        .body
        .iter()
        .filter(|statement| {
            matches!(
                &statement.kind,
                StatementKind::OpenScreen { screen } if screen == "interlude"
            )
        })
        .count();
    if expected_interludes != actual_interludes {
        bail!(
            "{} interlude mismatch for source {}: expected {}, found {}",
            path.display(),
            chapter.source_name,
            expected_interludes,
            actual_interludes
        );
    }
    verify_silence_holds(path, chapter, &story.body)?;
    verify_structural_turn(path, chapter, &story.body)?;
    verify_legacy_stage_direction(path, chapter, &story.body)?;
    Ok(())
}

fn verify_silence_holds(
    path: &Path,
    chapter: &NovelChapter,
    statements: &[Statement],
) -> Result<()> {
    let expected_markers = chapter
        .beats
        .iter()
        .filter(|beat| matches!(beat, NovelBeat::Reading { text, .. } if is_explicit_silence(text)))
        .count();
    let mut actual_markers = 0;
    for (index, statement) in statements.iter().enumerate() {
        let text = match &statement.kind {
            StatementKind::Narrate { text } | StatementKind::Say { text, .. } => text,
            _ => continue,
        };
        if !is_explicit_silence(text) {
            continue;
        }
        actual_markers += 1;
        let following = &statements[index.saturating_add(1)..];
        if !matches!(
            following.first().map(|statement| &statement.kind),
            Some(StatementKind::AwaitAdvance)
        ) || !matches!(
            following.get(1).map(|statement| &statement.kind),
            Some(StatementKind::ClearDialogue)
        ) {
            bail!(
                "{} does not hold explicit silence '{}' after source {}",
                path.display(),
                text,
                chapter.source_name
            );
        }
        if chapter.source_name == "10_day10.md" && text == "……" {
            let has_final_tint = matches!(
                following.get(2).map(|statement| &statement.kind),
                Some(StatementKind::Effect {
                    kind,
                    color,
                    amount,
                    duration_ms,
                    ..
                }) if kind == "tint" && color == "#05070b" && (*amount - 64.0).abs() < f64::EPSILON && *duration_ms == 520
            );
            let has_final_wait = matches!(
                following.get(3).map(|statement| &statement.kind),
                Some(StatementKind::Wait { duration_ms }) if *duration_ms == 520
            );
            if !has_final_tint || !has_final_wait {
                bail!(
                    "{} does not preserve Day 10's final dark hold",
                    path.display()
                );
            }
        } else if !matches!(
            following.get(2).map(|statement| &statement.kind),
            Some(StatementKind::Wait { duration_ms }) if *duration_ms == 420
        ) {
            bail!(
                "{} does not preserve the 420ms silence after '{}'",
                path.display(),
                text
            );
        }
    }
    if actual_markers != expected_markers {
        bail!(
            "{} explicit-silence marker mismatch for source {}: expected {}, found {}",
            path.display(),
            chapter.source_name,
            expected_markers,
            actual_markers
        );
    }
    Ok(())
}

fn verify_structural_turn(
    path: &Path,
    chapter: &NovelChapter,
    statements: &[Statement],
) -> Result<()> {
    let expected_breaks = chapter
        .beats
        .iter()
        .filter(|beat| matches!(beat, NovelBeat::StructuralBreak))
        .count();
    if expected_breaks == 0 {
        return Ok(());
    }
    if chapter.source_name != "00_init.md" || expected_breaks != 1 {
        bail!(
            "no Umikaze structural-turn mapping is defined for {}",
            chapter.source_name
        );
    }
    let has_pov_turn = statements.windows(3).any(|window| {
        matches!(&window[0].kind, StatementKind::ClearDialogue)
            && is_background_fade(&window[1], "#284b59", 360)
            && matches!(&window[2].kind, StatementKind::Wait { duration_ms: 360 })
    });
    if !has_pov_turn {
        bail!(
            "{} does not preserve Day 0's quiet POV transition",
            path.display()
        );
    }
    Ok(())
}

fn verify_legacy_stage_direction(
    path: &Path,
    chapter: &NovelChapter,
    statements: &[Statement],
) -> Result<()> {
    let expected_directions = chapter
        .beats
        .iter()
        .filter(|beat| matches!(beat, NovelBeat::Direction(_)))
        .count();
    if expected_directions == 0 {
        return Ok(());
    }
    if chapter.source_name != "05_day5.md" || expected_directions != 4 {
        bail!(
            "no Umikaze stage-direction mapping is defined for {}",
            chapter.source_name
        );
    }
    let has_hospital_turn = statements.windows(5).any(|window| {
        is_background_fade(&window[0], "#05070b", 2_000)
            && matches!(
                &window[1].kind,
                StatementKind::Stop { bus, .. } if matches!(bus, aria_core::modern::AudioBus::Bgm)
            )
            && matches!(&window[2].kind, StatementKind::Wait { duration_ms: 2_000 })
            && is_background_fade(&window[3], "#ded7c9", 420)
            && matches!(&window[4].kind, StatementKind::Wait { duration_ms: 180 })
    });
    if !has_hospital_turn {
        bail!(
            "{} does not preserve Day 5's authored fade/wait/fade transition",
            path.display()
        );
    }
    Ok(())
}

fn is_background_fade(statement: &Statement, path: &str, duration_ms: u32) -> bool {
    matches!(
        &statement.kind,
        StatementKind::Background {
            asset,
            transition: Some(transition),
        } if asset.path == path
            && matches!(transition.kind, aria_core::modern::TransitionKind::Fade)
            && transition.duration_ms == Some(duration_ms)
    )
}

fn umikaze_style_for(chapter: &NovelChapter) -> Result<&'static UmikazeChapterStyle> {
    UMIKAZE_CHAPTER_STYLES
        .iter()
        .find(|style| style.source_name == chapter.source_name)
        .with_context(|| {
            format!(
                "'{}' is not a canonical Umikaze Day 0–10 source",
                chapter.source_name
            )
        })
}

fn render_text_beat(output: &mut String, speaker: Option<&str>, text: &str) {
    if let Some(speaker) = speaker {
        output.push_str(&format!("  say {speaker}: \"{}\";\n", escape_string(text)));
    } else {
        output.push_str(&format!("  narrate \"{}\";\n", escape_string(text)));
    }
    output.push_str("  await advance;\n");
}

fn render_plain_direction(output: &mut String, direction: &NovelDirection) {
    match direction {
        NovelDirection::FadeOut { duration_ms } => {
            output.push_str("  clear dialogue;\n");
            output.push_str(&format!("  wait {duration_ms}ms;\n"));
        }
        NovelDirection::StopBgm => output.push_str("  stop bgm;\n"),
        NovelDirection::FadeIn => {}
        NovelDirection::Wait { duration_ms } => {
            output.push_str(&format!("  wait {duration_ms}ms;\n"))
        }
    }
}

fn render_umikaze_beat(
    output: &mut String,
    chapter: &NovelChapter,
    style: &UmikazeChapterStyle,
    beat: &NovelBeat,
) {
    match beat {
        NovelBeat::Reading { speaker, text } => {
            render_text_beat(output, speaker.as_deref(), text);
            if is_explicit_silence(text) {
                output.push_str("  clear dialogue;\n");
                if chapter.source_name == "10_day10.md" && text == "……" {
                    output.push_str("  effect tint \"#05070b\" amount 64 over 520ms;\n");
                    output.push_str("  wait 520ms;\n");
                } else {
                    output.push_str("  wait 420ms;\n");
                }
            }
        }
        NovelBeat::Heading { text } => {
            output.push_str("  clear dialogue;\n");
            output.push_str("  screen interlude;\n");
            output.push_str(&format!("  narrate \"{}\";\n", escape_string(text)));
            output.push_str("  wait 880ms;\n");
            output.push_str("  screen dialogue;\n");
            output.push_str("  clear dialogue;\n");
            output.push_str("  wait 220ms;\n");
        }
        NovelBeat::StructuralBreak => {
            output.push_str("  clear dialogue;\n");
            if chapter.source_name == "00_init.md" {
                output.push_str("  background asset(\"#284b59\") with fade(360ms);\n");
            } else {
                output.push_str(&format!(
                    "  background asset(\"{}\") with fade(320ms);\n",
                    style.background
                ));
            }
            output.push_str("  wait 360ms;\n");
        }
        NovelBeat::Direction(direction) => match direction {
            NovelDirection::FadeOut { duration_ms } => {
                output.push_str("  clear dialogue;\n");
                output.push_str(&format!(
                    "  background asset(\"#05070b\") with fade({duration_ms}ms);\n"
                ));
            }
            // A project may not have a BGM playing at this exact beat, but
            // the authored cue still matters: stopping the BGM bus makes the
            // following hold semantically silent instead of merely dark.
            NovelDirection::StopBgm => output.push_str("  stop bgm;\n"),
            NovelDirection::Wait { duration_ms } => {
                output.push_str(&format!("  wait {duration_ms}ms;\n"));
            }
            NovelDirection::FadeIn => {
                output.push_str("  background asset(\"#ded7c9\") with fade(420ms);\n");
                output.push_str("  wait 180ms;\n");
            }
        },
    }
}

fn is_explicit_silence(text: &str) -> bool {
    matches!(text.trim(), "..." | "……")
}

fn escape_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn is_aria_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let mut file = AtomicWriteFile::open(path)
        .with_context(|| format!("cannot open generated output {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("cannot write generated output {}", path.display()))?;
    file.as_file()
        .sync_all()
        .with_context(|| format!("cannot sync generated output {}", path.display()))?;
    file.commit()
        .with_context(|| format!("cannot commit generated output {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::compiler::{CompileInput, SourceUnit, compile};

    #[test]
    fn imports_canonical_beats_without_inventing_prose() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("00_init.md"),
            "**9月18日　保健室**\n\n俺「行こう」\n「うん」\n小さく「声」\n* * *\n# 9/18 END\n",
        )
        .unwrap();
        fs::write(
            source.join("01_start.md"),
            "朝の駅。\n\nミオ「\"海\"へ」\n;day1 end\n",
        )
        .unwrap();
        let output = temp.path().join("generated/ja-JP.aria");

        let report = import_novel(&source, &output, "chapter_select_ja", "ja-JP").unwrap();
        assert_eq!(report.chapters.len(), 2);
        assert_eq!(report.reading_beats, 6);
        assert_eq!(report.structural_breaks, 1);
        assert_eq!(report.stage_directions, 0);
        assert_eq!(report.chapters[0].label, "序章");
        assert_eq!(report.chapters[1].label, "DAY 1");

        let generated = fs::read_to_string(&output).unwrap();
        assert!(generated.contains("narrate \"9月18日　保健室\";"));
        assert!(generated.contains("say 俺: \"「行こう」\";"));
        assert!(generated.contains("narrate \"「うん」\";"));
        assert!(generated.contains("say ミオ: \"「\\\"海\\\"へ」\";"));
        assert!(generated.contains("narrate \"小さく「声」\";"));
        assert!(generated.contains("clear dialogue;\n  wait 550ms;"));
        assert!(!generated.contains("9/18 END"));
        assert!(!generated.contains("day1 end"));

        let output = compile(CompileInput {
            game_id: "jp.example.import".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![
                SourceUnit {
                    logical_path: "scripts/main.aria".to_owned(),
                    source: "aria;\nuse \"scenario/ja-JP.aria\";\nentry start;\nscene start { jump chapter_select_ja; }\n".to_owned(),
                },
                SourceUnit {
                    logical_path: "scripts/scenario/ja-JP.aria".to_owned(),
                    source: generated,
                },
            ],
        });
        assert!(!output.has_errors(), "{:#?}", output.diagnostics);
    }

    #[test]
    fn umikaze_profile_embeds_day_zero_to_ten_and_translates_stage_cues() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        for style in &UMIKAZE_CHAPTER_STYLES {
            fs::write(source.join(style.source_name), "本文。\n").unwrap();
        }
        fs::write(
            source.join("00_init.md"),
            "**9月18日　保健室**\n\n本文。\n\n# side2\n\n...\n",
        )
        .unwrap();
        fs::write(
            source.join("05_day5.md"),
            "雨の夜。\n; 暗転・雨の音フェードアウト\nfadeout 2\nstopbgm\nwait 2000\n; 病院回想\nfadein\n白い天井。\n",
        )
        .unwrap();
        fs::write(source.join("10_day10.md"), "終わり。\n……\n").unwrap();
        let output_path = temp.path().join("generated/umikaze.aria");
        let include = UMIKAZE_CHAPTER_STYLES
            .iter()
            .map(|style| style.source_name.to_owned())
            .collect();

        let report = import_novel_with_options(
            &source,
            &output_path,
            NovelImportOptions {
                chapter_select: "chapter_select_ja".to_owned(),
                locale: "ja-JP".to_owned(),
                include,
                presentation: NovelPresentation::Umikaze,
                layout: NovelImportLayout::Single,
            },
        )
        .unwrap();
        assert_eq!(report.chapters.len(), 11);
        assert_eq!(report.structural_breaks, 1);
        assert_eq!(report.stage_directions, 4);

        let generated = fs::read_to_string(&output_path).unwrap();
        assert!(generated.contains("module umikaze.scenario.ja.canonical;"));
        assert!(generated.contains("screen day_card;"));
        assert!(generated.contains("screen interlude;"));
        assert!(generated.contains("narrate \"9月18日　保健室\";"));
        assert!(generated.contains("background asset(\"#05070b\") with fade(2000ms);"));
        assert!(generated.contains("background asset(\"#ded7c9\") with fade(420ms);"));
        assert!(generated.contains("effect tint \"#05070b\" amount 64 over 520ms;"));
        assert!(!generated.contains("fadeout 2"));
        assert!(!generated.contains("病院回想"));

        let compiled = compile(CompileInput {
            game_id: "jp.example.umikaze-import".to_owned(),
            entry: "scripts/main.aria".to_owned(),
            sources: vec![
                SourceUnit {
                    logical_path: "scripts/main.aria".to_owned(),
                    source: "aria;\nuse \"scenario/umikaze.aria\";\nentry start;\nscene start { jump chapter_select_ja; }\n".to_owned(),
                },
                SourceUnit {
                    logical_path: "scripts/scenario/umikaze.aria".to_owned(),
                    source: generated,
                },
            ],
        });
        assert!(!compiled.has_errors(), "{:#?}", compiled.diagnostics);
    }

    #[test]
    fn umikaze_chapter_layout_keeps_each_day_in_its_own_aria_file() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        for style in &UMIKAZE_CHAPTER_STYLES {
            fs::write(source.join(style.source_name), "本文。\n").unwrap();
        }
        let output = temp.path().join("scenario/ja-JP");
        let include = UMIKAZE_CHAPTER_STYLES
            .iter()
            .map(|style| style.source_name.to_owned())
            .collect();

        let report = import_novel_with_options(
            &source,
            &output,
            NovelImportOptions {
                chapter_select: "chapter_select_ja".to_owned(),
                locale: "ja-JP".to_owned(),
                include,
                presentation: NovelPresentation::Umikaze,
                layout: NovelImportLayout::Chapters,
            },
        )
        .unwrap();

        assert_eq!(report.layout, NovelImportLayout::Chapters);
        let index = fs::read_to_string(output.join("index.aria")).unwrap();
        assert!(index.contains("use \"chapter-00.aria\";"));
        assert!(index.contains("use \"chapter-10.aria\";"));
        for chapter in 0..=10 {
            let source = fs::read_to_string(output.join(format!("chapter-{chapter:02}.aria")))
                .unwrap_or_else(|_| panic!("missing chapter-{chapter:02}.aria"));
            assert!(source.contains(&format!("module umikaze.scenario.ja.chapter_{chapter:02};")));
            assert!(source.contains(&format!("scene novel_chapter_{chapter:02} {{")));
            assert!(source.contains("screen day_card;"));
        }
    }

    #[test]
    fn verifier_rejects_a_manual_visible_text_change_without_rewriting_it() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        for style in &UMIKAZE_CHAPTER_STYLES {
            fs::write(source.join(style.source_name), "本文。\n").unwrap();
        }
        let output = temp.path().join("scenario/ja-JP");
        let options = NovelImportOptions {
            chapter_select: "chapter_select_ja".to_owned(),
            locale: "ja-JP".to_owned(),
            include: UMIKAZE_CHAPTER_STYLES
                .iter()
                .map(|style| style.source_name.to_owned())
                .collect(),
            presentation: NovelPresentation::Umikaze,
            layout: NovelImportLayout::Chapters,
        };
        import_novel_with_options(&source, &output, options.clone()).unwrap();

        let day_ten = output.join("chapter-10.aria");
        let modified = fs::read_to_string(&day_ten).unwrap().replacen(
            "narrate \"本文。\";",
            "narrate \"改ざん。\";",
            1,
        );
        fs::write(&day_ten, &modified).unwrap();

        let error = verify_novel_output(&source, &output, options).unwrap_err();
        assert!(error.to_string().contains("player-text mismatch"));
        assert!(fs::read_to_string(day_ten).unwrap().contains("改ざん。"));
    }

    #[test]
    fn verifier_rejects_a_changed_authored_silence_hold() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        for style in &UMIKAZE_CHAPTER_STYLES {
            fs::write(source.join(style.source_name), "本文。\n").unwrap();
        }
        fs::write(source.join("00_init.md"), "...\n").unwrap();
        let output = temp.path().join("scenario/ja-JP");
        let options = NovelImportOptions {
            chapter_select: "chapter_select_ja".to_owned(),
            locale: "ja-JP".to_owned(),
            include: UMIKAZE_CHAPTER_STYLES
                .iter()
                .map(|style| style.source_name.to_owned())
                .collect(),
            presentation: NovelPresentation::Umikaze,
            layout: NovelImportLayout::Chapters,
        };
        import_novel_with_options(&source, &output, options.clone()).unwrap();

        let day_zero = output.join("chapter-00.aria");
        let modified =
            fs::read_to_string(&day_zero)
                .unwrap()
                .replacen("wait 420ms;", "wait 1ms;", 1);
        fs::write(&day_zero, modified).unwrap();

        let error = verify_novel_output(&source, &output, options).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not preserve the 420ms silence")
        );
    }

    #[test]
    fn explicit_include_excludes_unselected_markdown_chapters() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("00_init.md"), "序章。\n").unwrap();
        fs::write(source.join("01_start.md"), "一日目。\n").unwrap();
        fs::write(source.join("14_day14.md"), "未公開の草稿。\n").unwrap();
        let output = temp.path().join("out.aria");

        let report = import_novel_with_options(
            &source,
            &output,
            NovelImportOptions {
                chapter_select: "chapter_select_ja".to_owned(),
                locale: "ja-JP".to_owned(),
                include: vec!["00_init.md".to_owned(), "01_start.md".to_owned()],
                presentation: NovelPresentation::Plain,
                layout: NovelImportLayout::Single,
            },
        )
        .unwrap();

        assert_eq!(report.chapters.len(), 2);
        let generated = fs::read_to_string(output).unwrap();
        assert!(generated.contains("序章。"));
        assert!(generated.contains("一日目。"));
        assert!(!generated.contains("未公開の草稿。"));
    }

    #[test]
    fn rejects_an_identifier_that_could_inject_story_source() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("00_init.md"), "本文。\n").unwrap();

        let error = import_novel(
            &source,
            &temp.path().join("out.aria"),
            "chapter; end",
            "ja-JP",
        )
        .unwrap_err();
        assert!(error.to_string().contains("Aria identifier"));
    }
}
