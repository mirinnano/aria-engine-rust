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
    /// Labels that mark a line such as `名前「…」` as dialogue by that
    /// speaker. Any other prefix stays part of the narration.
    pub speakers: Vec<String>,
}

impl NovelImportOptions {
    pub fn plain(chapter_select: impl Into<String>, locale: impl Into<String>) -> Self {
        Self {
            chapter_select: chapter_select.into(),
            locale: locale.into(),
            include: Vec::new(),
            speakers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NovelImportReport {
    pub source_directory: String,
    pub output: String,
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
    /// A source line which is still part of the canonical prose, but whose
    /// delivery belongs to the story rather than the reader.  It is rendered
    /// atomically on an unadorned solid field and advances on its authored
    /// timer; it is never duplicated as a subtitle.
    Statement {
        text: String,
        field: String,
    },
    Heading {
        text: String,
    },
    /// A blank Markdown paragraph is kept as a short, non-verbal breath after
    /// the player has released the preceding sentence.  Keeping it as a semantic beat means prose stays
    /// canonical while its written spacing can still shape the reading pace.
    Breath {
        duration_ms: u32,
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

/// A staging cue written in the Markdown. It is not prose, so it never
/// appears in the reader as a literal command; the importer keeps only its
/// timing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NovelDirection {
    FadeOut {
        duration_ms: u32,
    },
    StopBgm,
    Wait {
        duration_ms: u32,
    },
    FadeIn,
    /// An authored absence: remove both dialogue and location for a measured
    /// hold, then return to the chapter's present scene.  `ash` is deliberately
    /// a visual term rather than a generic effect name; it is the quiet grey
    /// field used when a landscape would over-explain a thought.
    AshPause {
        duration_ms: u32,
    },
    /// A deliberate beat after a reader has released the preceding line.
    /// The source names it either `breath` or `drift`; runtime work is the
    /// same quiet, player-independent hold, while the distinction remains
    /// useful and reviewable in the manuscript.
    QuietPause {
        duration_ms: u32,
    },
    /// A location or weather turn with no new prose.  The finite tone list
    /// prevents a manuscript from acquiring arbitrary visual asset paths.
    Backdrop {
        tone: String,
    },
}

/// A directive name such as `rail` in `backdrop rail` or `sea` in
/// `statement sea`. The importer only checks its spelling; what it looks like
/// belongs to the project that reads the generated source.
fn directive_name(value: &str) -> Option<String> {
    let value = value.trim();
    is_aria_identifier(value).then(|| value.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NovelControl {
    ExplicitPacing,
    Statement(String),
    Direction(NovelDirection),
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

    let generated = render_plain_module(&chapters, &options.chapter_select, &options.locale);
    verify_generated_source(out, &generated)?;
    write_atomic(out, generated.as_bytes())?;

    verify_existing_output(out, &chapters)?;
    Ok(novel_import_report(&source_directory, out, &chapters))
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
    verify_existing_output(out, &chapters)?;
    Ok(novel_import_report(&source_directory, out, &chapters))
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

    let chapters = discover_chapters(&source_directory, &options.include, &options.speakers)?;
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
        .filter(|beat| matches!(beat, NovelBeat::Direction(_) | NovelBeat::Statement { .. }))
        .count();

    NovelImportReport {
        source_directory: source_directory.display().to_string(),
        output: out.display().to_string(),
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

fn discover_chapters(
    source_directory: &Path,
    include: &[String],
    speakers: &[String],
) -> Result<Vec<NovelChapter>> {
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
        .map(|(index, source_path)| parse_chapter(index, source_path, speakers))
        .collect()
}

fn parse_chapter(index: usize, source_path: PathBuf, speakers: &[String]) -> Result<NovelChapter> {
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
    let beats = parse_beats(&source, speakers)?;
    if !beats.iter().any(is_player_text) {
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

fn parse_beats(source: &str, speakers: &[String]) -> Result<Vec<NovelBeat>> {
    let mut beats = Vec::new();
    let mut follows_paragraph_gap = false;
    let mut explicit_pacing = false;
    let mut pending_statement = None;

    for (line_index, source_line) in source.lines().enumerate() {
        let line_number = line_index.saturating_add(1);
        let line = source_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank space only becomes time when it follows readable prose.
            // Multiple blank lines intentionally remain one breath.
            follows_paragraph_gap |= matches!(beats.last(), Some(NovelBeat::Reading { .. }));
            continue;
        }
        if is_day_end_marker(trimmed) {
            follows_paragraph_gap = false;
            continue;
        }
        if trimmed.starts_with(';') {
            // Source comments annotate a beat but do not consume the blank
            // paragraph boundary before it.
            continue;
        }

        if let Some(control) = parse_control(trimmed)? {
            match control {
                NovelControl::ExplicitPacing => {
                    explicit_pacing = true;
                    follows_paragraph_gap = false;
                }
                NovelControl::Statement(field) => {
                    if pending_statement.replace(field).is_some() {
                        bail!("authored statement at line {line_number} has no prose line to own");
                    }
                    follows_paragraph_gap = false;
                }
                NovelControl::Direction(direction) => {
                    if pending_statement.is_some() {
                        bail!(
                            "authored statement before line {line_number} must be followed by one prose line"
                        );
                    }
                    follows_paragraph_gap = false;
                    beats.push(NovelBeat::Direction(direction));
                }
            }
            continue;
        }

        let mut beat = if trimmed == "* * *" {
            NovelBeat::StructuralBreak
        } else if trimmed.starts_with("# ") {
            // `# side2` in the canonical manuscript is a source-side POV
            // divider, not player-facing prose. Treat all remaining Markdown
            // headings as a silent structural turn; authored date headings
            // use `**...**` and remain visible below.
            NovelBeat::StructuralBreak
        } else if let Some(text) = strip_scene_heading(line) {
            NovelBeat::Heading { text }
        } else {
            let (speaker, text) = split_attributed_dialogue(line, speakers).map_or_else(
                || (None, line.to_owned()),
                |(speaker, text)| (Some(speaker), text),
            );
            NovelBeat::Reading { speaker, text }
        };

        if let Some(field) = pending_statement.take() {
            beat = match beat {
                NovelBeat::Reading {
                    speaker: None,
                    text,
                } => NovelBeat::Statement { text, field },
                _ => {
                    bail!(
                        "authored statement before line {line_number} must own one unattributed prose line"
                    )
                }
            };
        }

        if !explicit_pacing
            && follows_paragraph_gap
            && matches!(&beat, NovelBeat::Reading { .. })
            && let Some(NovelBeat::Reading { text, .. }) = beats.last()
            && !is_explicit_silence(text)
        {
            beats.push(NovelBeat::Breath {
                duration_ms: paragraph_breath_duration(text),
            });
        }
        follows_paragraph_gap = false;
        beats.push(beat);
    }

    if pending_statement.is_some() {
        bail!("authored statement must be followed by one unattributed prose line");
    }

    // A sentence can carry its own small breath even when the source opts
    // into explicit paragraph pacing.  Blank paragraphs remain author-owned
    // in `pacing explicit`, but terminal punctuation still gives each player
    // beat a legible release instead of an immediate cut to the next line.
    Ok(with_semantic_breaths(beats))
}

fn parse_control(line: &str) -> Result<Option<NovelControl>> {
    let command = line.trim();
    let normalized = command.to_ascii_lowercase();
    if normalized == "pacing explicit" {
        return Ok(Some(NovelControl::ExplicitPacing));
    }
    if normalized == "pacing" || normalized.starts_with("pacing ") {
        bail!("unsupported pacing directive '{command}'; use 'pacing explicit'");
    }
    if normalized == "stopbgm" {
        return Ok(Some(NovelControl::Direction(NovelDirection::StopBgm)));
    }
    if normalized == "fadein" {
        return Ok(Some(NovelControl::Direction(NovelDirection::FadeIn)));
    }
    if let Some(seconds) = normalized.strip_prefix("fadeout ") {
        let seconds = parse_control_duration("fadeout", seconds)?;
        let duration_ms = seconds
            .checked_mul(1_000)
            .context("fadeout duration exceeds the supported millisecond range")?;
        return Ok(Some(NovelControl::Direction(NovelDirection::FadeOut {
            duration_ms,
        })));
    }
    if let Some(milliseconds) = normalized.strip_prefix("wait ") {
        let duration_ms = parse_control_duration("wait", milliseconds)?;
        return Ok(Some(NovelControl::Direction(NovelDirection::Wait {
            duration_ms,
        })));
    }
    for prefix in ["pause ash ", "pause black "] {
        if let Some(milliseconds) = normalized.strip_prefix(prefix) {
            let duration_ms = parse_control_duration(prefix.trim_end(), milliseconds)?;
            return Ok(Some(NovelControl::Direction(NovelDirection::AshPause {
                duration_ms,
            })));
        }
    }
    if let Some(rest) = normalized.strip_prefix("pause ") {
        let mut parts = rest.split_whitespace();
        let style = parts.next().unwrap_or_default();
        let duration = parts.next().unwrap_or_default();
        if parts.next().is_some() || !matches!(style, "breath" | "drift") {
            bail!(
                "unsupported pause directive '{command}'; use 'pause breath|drift|ash|black <milliseconds>'"
            );
        }
        let duration_ms = parse_control_duration(&format!("pause {style}"), duration)?;
        return Ok(Some(NovelControl::Direction(NovelDirection::QuietPause {
            duration_ms,
        })));
    }
    if let Some(tone) = normalized.strip_prefix("backdrop ") {
        let Some(tone) = directive_name(tone) else {
            bail!("backdrop directive '{command}' needs one lowercase name");
        };
        return Ok(Some(NovelControl::Direction(NovelDirection::Backdrop {
            tone,
        })));
    }
    if let Some(field) = normalized.strip_prefix("statement ") {
        let Some(field) = directive_name(field) else {
            bail!("statement directive '{command}' needs one lowercase name");
        };
        return Ok(Some(NovelControl::Statement(field)));
    }
    if matches!(normalized.as_str(), "pause" | "backdrop" | "statement") {
        bail!("incomplete authored stage directive '{command}'");
    }
    Ok(None)
}

fn parse_control_duration(label: &str, value: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .with_context(|| format!("{label} requires a non-negative millisecond value"))
}

/// Returns the automatic breath for a sentence boundary.  The values are
/// intentionally separated from authored `pause` directives: this is the
/// reader's ordinary respiration, not a plot-specific hold.
fn natural_breath_duration(text: &str) -> u32 {
    let text = text.trim();
    if text.ends_with('…') || text.ends_with("...") || text.ends_with("――") {
        480
    } else if text.ends_with(['。', '！', '？', '!', '?', '」', '』']) {
        220
    } else {
        0
    }
}

fn paragraph_breath_duration(text: &str) -> u32 {
    natural_breath_duration(text).max(300)
}

fn with_semantic_breaths(beats: Vec<NovelBeat>) -> Vec<NovelBeat> {
    let mut paced = Vec::with_capacity(beats.len().saturating_mul(2));
    for (index, beat) in beats.iter().enumerate() {
        paced.push(beat.clone());
        let NovelBeat::Reading { text, .. } = beat else {
            continue;
        };
        if is_explicit_silence(text) || natural_breath_duration(text) == 0 {
            continue;
        }
        // An authored pause or an existing paragraph breath owns this edge;
        // do not stack two waits at the same semantic boundary.
        let next = beats.get(index + 1);
        let owned_by_author = matches!(
            next,
            Some(NovelBeat::Breath { .. })
                | Some(NovelBeat::Direction(
                    NovelDirection::AshPause { .. }
                        | NovelDirection::QuietPause { .. }
                        | NovelDirection::Wait { .. }
                        | NovelDirection::Backdrop { .. }
                        | NovelDirection::FadeOut { .. }
                        | NovelDirection::FadeIn,
                ))
        );
        if !owned_by_author && matches!(next, Some(NovelBeat::Reading { .. })) {
            paced.push(NovelBeat::Breath {
                duration_ms: natural_breath_duration(text),
            });
        }
    }
    paced
}

fn is_player_text(beat: &NovelBeat) -> bool {
    matches!(
        beat,
        NovelBeat::Reading { .. } | NovelBeat::Statement { .. } | NovelBeat::Heading { .. }
    )
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
fn split_attributed_dialogue(line: &str, speakers: &[String]) -> Option<(String, String)> {
    let (speaker, spoken) = line.split_once('「')?;
    if speaker.is_empty()
        || !speakers.iter().any(|known| known == speaker)
        || !spoken.ends_with('」')
    {
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
                NovelBeat::Statement { text, .. } => {
                    // A statement advances on its own timer. How it looks
                    // (for example a dedicated screen) is up to the project.
                    output.push_str("  clear dialogue;\n");
                    output.push_str(&format!("  narrate \"{}\";\n", escape_string(text)));
                    output.push_str("  wait 1600ms;\n");
                    output.push_str("  clear dialogue;\n");
                }
                NovelBeat::Heading { text } => {
                    render_text_beat(&mut output, None, text);
                }
                NovelBeat::Breath { duration_ms } => {
                    output.push_str("  clear dialogue;\n");
                    output.push_str(&format!("  wait breath {duration_ms}ms;\n"));
                }
                NovelBeat::StructuralBreak => {
                    output.push_str("  clear dialogue;\n");
                    output.push_str("  wait 700ms;\n");
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

fn verify_existing_output(out: &Path, chapters: &[NovelChapter]) -> Result<()> {
    for chapter in chapters {
        verify_chapter_prose(out, chapter)?;
    }
    Ok(())
}

fn verify_chapter_prose(path: &Path, chapter: &NovelChapter) -> Result<()> {
    let scene_name = &chapter.scene;
    let module = parse_generated_module(path)?;
    let scene = module
        .scenes
        .iter()
        .find(|scene| &scene.name == scene_name)
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
            NovelBeat::Statement { text, .. } => Some(PlayerText {
                speaker: None,
                text: text.clone(),
            }),
            NovelBeat::Heading { text } => Some(PlayerText {
                speaker: None,
                text: text.clone(),
            }),
            NovelBeat::Breath { .. } | NovelBeat::StructuralBreak | NovelBeat::Direction(_) => None,
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
        NovelDirection::AshPause { duration_ms, .. } => {
            output.push_str("  clear dialogue;\n");
            output.push_str(&format!("  wait {duration_ms}ms;\n"));
        }
        NovelDirection::QuietPause { duration_ms } => {
            output.push_str("  clear dialogue;\n");
            output.push_str(&format!("  wait {duration_ms}ms;\n"));
        }
        NovelDirection::Backdrop { .. } => {
            output.push_str("  clear dialogue;\n");
            output.push_str("  wait 280ms;\n");
        }
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

        let report = import_novel_with_options(
            &source,
            &output,
            NovelImportOptions {
                speakers: vec!["俺".to_owned(), "ミオ".to_owned()],
                ..NovelImportOptions::plain("chapter_select_ja", "ja-JP")
            },
        )
        .unwrap();
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
        assert!(generated.contains("clear dialogue;\n  wait 700ms;"));
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
    fn blank_paragraphs_and_ash_cues_are_distinct_timed_beats() {
        let beats = parse_beats(
            "最初の文。\n\n余韻……\n\n次の文。\n\npause ash 720\n\nその後。\n",
            &[],
        )
        .unwrap();

        assert_eq!(
            beats,
            vec![
                NovelBeat::Reading {
                    speaker: None,
                    text: "最初の文。".to_owned(),
                },
                NovelBeat::Breath { duration_ms: 300 },
                NovelBeat::Reading {
                    speaker: None,
                    text: "余韻……".to_owned(),
                },
                NovelBeat::Breath { duration_ms: 480 },
                NovelBeat::Reading {
                    speaker: None,
                    text: "次の文。".to_owned(),
                },
                NovelBeat::Direction(NovelDirection::AshPause { duration_ms: 720 }),
                NovelBeat::Reading {
                    speaker: None,
                    text: "その後。".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn explicit_pacing_keeps_blank_markdown_inert_and_assigns_each_stage_cue() {
        let beats = parse_beats(
            "pacing explicit\n\n前の文。\n\npause breath 240\n\nbackdrop rail\n\nstatement sea\n\nマイクの先で、風が鳴った。\n\n次の文。\n",
            &[],
        )
        .unwrap();

        assert_eq!(
            beats,
            vec![
                NovelBeat::Reading {
                    speaker: None,
                    text: "前の文。".to_owned(),
                },
                NovelBeat::Direction(NovelDirection::QuietPause { duration_ms: 240 }),
                NovelBeat::Direction(NovelDirection::Backdrop {
                    tone: "rail".to_owned(),
                }),
                NovelBeat::Statement {
                    text: "マイクの先で、風が鳴った。".to_owned(),
                    field: "sea".to_owned(),
                },
                NovelBeat::Reading {
                    speaker: None,
                    text: "次の文。".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn verifier_rejects_a_manual_visible_text_change_without_rewriting_it() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("00_init.md"), "本文。\n").unwrap();
        let output = temp.path().join("story.aria");
        let options = NovelImportOptions::plain("chapter_select_ja", "ja-JP");
        import_novel_with_options(&source, &output, options.clone()).unwrap();

        let original = fs::read_to_string(&output).unwrap();
        let modified = original.replacen("\"本文。\"", "\"改ざん。\"", 1);
        assert_ne!(
            original, modified,
            "the fixture line must be present to tamper with"
        );
        fs::write(&output, &modified).unwrap();

        let error = verify_novel_output(&source, &output, options).unwrap_err();
        assert!(error.to_string().contains("player-text mismatch"));
        assert!(fs::read_to_string(output).unwrap().contains("改ざん。"));
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
                speakers: Vec::new(),
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
