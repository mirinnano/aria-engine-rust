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
    let lines = wrap_japanese(text, max_cells.max(1));
    let lines_per_page = lines_per_page.max(1);
    if lines.is_empty() {
        return vec![String::new()];
    }
    lines
        .chunks(lines_per_page)
        .map(|page| page.join("\n"))
        .collect()
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
}
