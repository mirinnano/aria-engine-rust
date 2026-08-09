use unicode_segmentation::UnicodeSegmentation;

const FORBIDDEN_LINE_START: &str =
    "、。，．・：；？！ー〜…‥ヽヾゝゞ々〻）］｝〕〉》」』】〙〗〟’”｠»";
const FORBIDDEN_LINE_END: &str = "（［｛〔〈《「『【〘〖〝‘“｟«";

/// Wraps text without splitting grapheme clusters and applies basic Japanese
/// line-start/line-end prohibition rules. Width is measured in display cells:
/// ASCII uses one cell and non-ASCII graphemes use two.
///
/// Unlike the historical helper, this never extends a line past `max_cells`
/// to rescue a prohibited line start. It moves the break back instead. This
/// matters for the fixed subtitle grid: a correct kinsoku result that spills
/// out of the black band is still an incorrect reading surface.
#[must_use]
pub fn wrap_japanese(text: &str, max_cells: usize) -> Vec<String> {
    if max_cells == 0 {
        return vec![text.to_owned()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let graphemes = UnicodeSegmentation::graphemes(paragraph, true).collect::<Vec<_>>();
        let mut start = 0;
        while start < graphemes.len() {
            let mut end = start;
            let mut width = 0;
            while end < graphemes.len() {
                let next = display_cells(graphemes[end]);
                if end > start && width + next > max_cells {
                    break;
                }
                width += next;
                end += 1;
            }
            if end == start {
                end += 1;
            }

            if end < graphemes.len() {
                end = choose_line_break(&graphemes, start, end);
            }
            lines.push(graphemes[start..end].concat());
            start = end;
        }
    }
    lines
}

/// Splits a source line into fixed-height subtitle pages.  Every newline in
/// the result is explicit and no page contains more than `lines_per_page`
/// physical lines, so a host never needs a browser-dependent balancing pass.
#[must_use]
pub fn paginate_subtitles(text: &str, max_cells: usize, lines_per_page: usize) -> Vec<String> {
    paginate_subtitles_spanned(text, max_cells, lines_per_page)
        .into_iter()
        .map(|page| page.text)
        .collect()
}

/// A paginated subtitle page with a deterministic mapping from display
/// boundaries back to source grapheme boundaries. `source_end` may advance
/// beyond the final visible grapheme when an authored LF lands at a page
/// boundary and therefore has no rendered separator in this page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedSubtitlePage {
    pub text: String,
    pub source_start: usize,
    pub source_end: usize,
    display_source_boundaries: Vec<usize>,
}

impl SpannedSubtitlePage {
    #[must_use]
    pub fn source_boundary_at_display(&self, visible_graphemes: usize) -> usize {
        if visible_graphemes >= self.display_source_boundaries.len().saturating_sub(1) {
            self.source_end
        } else {
            self.display_source_boundaries[visible_graphemes]
        }
    }

    #[must_use]
    pub fn display_grapheme_count(&self) -> usize {
        self.display_source_boundaries.len().saturating_sub(1)
    }
}

/// Like [`paginate_subtitles`], while retaining source-boundary provenance for
/// reflow and save restoration. The string output is intentionally identical.
#[must_use]
pub fn paginate_subtitles_spanned(
    text: &str,
    max_cells: usize,
    lines_per_page: usize,
) -> Vec<SpannedSubtitlePage> {
    let lines = wrap_japanese_spanned(text, max_cells.max(1));
    let lines_per_page = lines_per_page.max(1);
    if lines.is_empty() {
        return vec![SpannedSubtitlePage {
            text: String::new(),
            source_start: 0,
            source_end: 0,
            display_source_boundaries: vec![0],
        }];
    }
    lines
        .chunks(lines_per_page)
        .map(|chunk| {
            let source_start = chunk.first().map_or(0, |line| line.source_start);
            let source_end = chunk.last().map_or(source_start, |line| line.source_end);
            let mut text = String::new();
            let mut boundaries = vec![source_start];
            for (index, line) in chunk.iter().enumerate() {
                let mut boundary = line.source_start;
                for grapheme in line.text.graphemes(true) {
                    text.push_str(grapheme);
                    boundary = boundary.saturating_add(1);
                    boundaries.push(boundary);
                }
                if index + 1 < chunk.len() {
                    text.push('\n');
                    // This display LF either represents an authored LF (the
                    // next source boundary advances) or a layout-only wrap.
                    boundaries.push(line.source_end);
                }
            }
            SpannedSubtitlePage {
                text,
                source_start,
                source_end,
                display_source_boundaries: boundaries,
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
struct SpannedLine {
    text: String,
    source_start: usize,
    source_end: usize,
}

fn wrap_japanese_spanned(text: &str, max_cells: usize) -> Vec<SpannedLine> {
    if max_cells == 0 {
        return vec![SpannedLine {
            text: text.to_owned(),
            source_start: 0,
            source_end: grapheme_count(text),
        }];
    }
    let mut lines = Vec::new();
    let paragraphs = text.split('\n').collect::<Vec<_>>();
    let mut source_cursor = 0_usize;
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let graphemes = UnicodeSegmentation::graphemes(*paragraph, true).collect::<Vec<_>>();
        let trailing_lf = usize::from(paragraph_index + 1 < paragraphs.len());
        if graphemes.is_empty() {
            lines.push(SpannedLine {
                text: String::new(),
                source_start: source_cursor,
                source_end: source_cursor.saturating_add(trailing_lf),
            });
            source_cursor = source_cursor.saturating_add(trailing_lf);
            continue;
        }
        let mut start = 0;
        while start < graphemes.len() {
            let mut end = start;
            let mut width = 0;
            while end < graphemes.len() {
                let next = display_cells(graphemes[end]);
                if end > start && width + next > max_cells {
                    break;
                }
                width += next;
                end += 1;
            }
            if end == start {
                end += 1;
            }
            if end < graphemes.len() {
                end = choose_line_break(&graphemes, start, end);
            }
            let source_start = source_cursor.saturating_add(start);
            let is_last_line = end == graphemes.len();
            lines.push(SpannedLine {
                text: graphemes[start..end].concat(),
                source_start,
                source_end: source_cursor
                    .saturating_add(end)
                    .saturating_add(usize::from(is_last_line) * trailing_lf),
            });
            start = end;
        }
        source_cursor = source_cursor
            .saturating_add(graphemes.len())
            .saturating_add(trailing_lf);
    }
    lines
}

#[must_use]
pub fn grapheme_count(text: &str) -> usize {
    UnicodeSegmentation::graphemes(text, true).count()
}

#[must_use]
pub fn grapheme_prefix(text: &str, count: usize) -> String {
    UnicodeSegmentation::graphemes(text, true)
        .take(count)
        .collect()
}

fn display_cells(grapheme: &str) -> usize {
    if grapheme.is_ascii() { 1 } else { 2 }
}

fn starts_with_forbidden(grapheme: &str, forbidden: &str) -> bool {
    grapheme
        .chars()
        .next()
        .is_some_and(|character| forbidden.contains(character))
}

fn choose_line_break(graphemes: &[&str], start: usize, fitted_end: usize) -> usize {
    let minimum = start + 1;
    let mut maximum = fitted_end;

    // Avoid a closing mark at the next line's start by moving the boundary
    // back. The mark stays within the next line after a normal grapheme,
    // rather than forcing an over-wide current line.
    while maximum > minimum
        && maximum < graphemes.len()
        && starts_with_forbidden(graphemes[maximum], FORBIDDEN_LINE_START)
    {
        maximum -= 1;
    }
    // Conversely, an opening mark belongs with the following character.
    while maximum > minimum && starts_with_forbidden(graphemes[maximum - 1], FORBIDDEN_LINE_END) {
        maximum -= 1;
    }

    // Prefer a natural sentence/phrase edge near the available width.  The
    // short search window prevents every comma in a long line from producing
    // a ragged, unnecessarily narrow subtitle.
    let preferred_start = maximum.saturating_sub(8).max(minimum);
    for candidate in (preferred_start..=maximum).rev() {
        if preferred_break_after(graphemes[candidate - 1])
            && (candidate == graphemes.len()
                || !starts_with_forbidden(graphemes[candidate], FORBIDDEN_LINE_START))
        {
            return candidate;
        }
    }
    maximum
}

fn preferred_break_after(grapheme: &str) -> bool {
    grapheme.chars().last().is_some_and(|character| {
        character.is_whitespace()
            || matches!(
                character,
                '、' | '。'
                    | '，'
                    | '．'
                    | ','
                    | '.'
                    | '!'
                    | '?'
                    | '！'
                    | '？'
                    | '：'
                    | '；'
                    | ':'
                    | ';'
                    | '—'
                    | '…'
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinsoku_keeps_closing_punctuation_off_line_start() {
        let lines = wrap_japanese("彼女は笑った。「また明日。」", 10);
        assert!(lines.iter().skip(1).all(|line| {
            !line
                .chars()
                .next()
                .is_some_and(|character| FORBIDDEN_LINE_START.contains(character))
        }));
        assert!(lines.iter().all(|line| {
            !line
                .chars()
                .last()
                .is_some_and(|character| FORBIDDEN_LINE_END.contains(character))
        }));
    }

    #[test]
    fn typewriter_does_not_split_emoji_grapheme() {
        let text = "海👩‍👩‍👧‍👦風";
        assert_eq!(grapheme_count(text), 3);
        assert_eq!(grapheme_prefix(text, 2), "海👩‍👩‍👧‍👦");
    }

    #[test]
    fn fixed_subtitle_pages_keep_two_lines_and_prioritize_punctuation() {
        let pages = paginate_subtitles(
            "海風が止んだ。次の駅まで、まだ時間がある。彼女は窓の外を見ていた。",
            18,
            2,
        );
        assert!(pages.iter().all(|page| page.lines().count() <= 2));
        assert!(pages.iter().flat_map(|page| page.lines()).all(|line| {
            line.chars().fold(0, |cells, character| {
                cells + if character.is_ascii() { 1 } else { 2 }
            }) <= 18
        }));
        assert!(
            pages
                .iter()
                .any(|page| page.contains("。\n") || page.ends_with('。'))
        );
    }

    #[test]
    fn long_multilingual_subtitle_never_requires_a_third_line() {
        let source = format!(
            "{} {} {}",
            "海風が止んだ。".repeat(35),
            "The tide carries every quiet sentence toward the next station.".repeat(4),
            "海風仍在窗外慢慢退去。".repeat(12),
        );
        let pages = paginate_subtitles(&source, 44, 2);
        assert!(pages.len() > 1);
        assert!(pages.iter().all(|page| page.lines().count() <= 2));
        assert!(pages.iter().flat_map(|page| page.lines()).all(|line| {
            UnicodeSegmentation::graphemes(line, true)
                .map(display_cells)
                .sum::<usize>()
                <= 44
        }));
    }

    #[test]
    fn spanned_pages_consume_authored_newlines_even_at_page_boundaries() {
        let boundary = paginate_subtitles_spanned("海風\n空", 4, 1);
        assert_eq!(
            boundary.iter().map(|page| &page.text).collect::<Vec<_>>(),
            vec!["海風", "空"]
        );
        assert_eq!(
            boundary[0].source_boundary_at_display(boundary[0].display_grapheme_count()),
            3
        );
        assert_eq!(boundary[1].source_start, 3);
        assert_eq!(boundary[1].source_end, 4);

        let consecutive = paginate_subtitles_spanned("海\n\n風", 4, 1);
        assert_eq!(
            consecutive
                .iter()
                .map(|page| &page.text)
                .collect::<Vec<_>>(),
            vec!["海", "", "風"]
        );
        assert_eq!(consecutive[0].source_end, 2);
        assert_eq!(consecutive[1].source_start, 2);
        assert_eq!(consecutive[1].source_end, 3);
        assert_eq!(consecutive[2].source_start, 3);
        assert_eq!(consecutive[2].source_end, 4);
    }
}
