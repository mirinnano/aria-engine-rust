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
    wrap_lines(text, max_cells, LineFill::Phrase)
}

/// How a line is filled. A subtitle band breaks at phrase edges and balances
/// a scrap; a book's page fills every line to the measure and moves a break
/// only for the line-start and line-end rules, so a short line reads as the
/// end of a paragraph and nothing else.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LineFill {
    Phrase,
    Full,
}

fn wrap_lines(text: &str, max_cells: usize, fill: LineFill) -> Vec<String> {
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
                end = choose_line_break(&graphemes, start, end, fill);
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
///
/// A paragraph (a run between source newlines) that fits on one page is not
/// split across two: when it does not fit in what is left of a page, it
/// begins the next one. The reader's advance then falls where the script
/// breaks, not inside a sentence. Only a paragraph longer than a page is cut.
///
/// A page of more than two lines is a book's page, not a subtitle band: its
/// lines are filled to the measure (`LineFill::Full`).
#[must_use]
pub fn paginate_subtitles(text: &str, max_cells: usize, lines_per_page: usize) -> Vec<String> {
    let fill = if lines_per_page > 2 {
        LineFill::Full
    } else {
        LineFill::Phrase
    };
    let lines_per_page = lines_per_page.max(1);
    let max_cells = max_cells.max(1);
    let mut pages: Vec<Vec<String>> = Vec::new();
    let mut page: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let lines = wrap_lines(paragraph, max_cells, fill);
        if !page.is_empty()
            && lines.len() <= lines_per_page
            && page.len() + lines.len() > lines_per_page
        {
            pages.push(std::mem::take(&mut page));
        }
        for line in lines {
            if page.len() == lines_per_page {
                pages.push(std::mem::take(&mut page));
            }
            page.push(line);
        }
    }
    if !page.is_empty() || pages.is_empty() {
        pages.push(page);
    }
    pages.into_iter().map(|page| page.join("\n")).collect()
}

/// Grapheme offsets where a line may break by this module's rules: after a
/// phrase-ending mark, or where kana meets kanji or katakana, and never where
/// kinsoku forbids it. A host that lays text out itself (a large centred
/// statement) offers only these breaks to the browser, so a word is never
/// split and WebKit and Chromium choose among the same places.
#[must_use]
pub fn phrase_break_opportunities(text: &str) -> Vec<u32> {
    let graphemes = UnicodeSegmentation::graphemes(text, true).collect::<Vec<_>>();
    (1..graphemes.len())
        .filter(|&index| {
            let before = graphemes[index - 1];
            let after = graphemes[index];
            !starts_with_forbidden(after, FORBIDDEN_LINE_START)
                && !starts_with_forbidden(before, FORBIDDEN_LINE_END)
                && !before.contains('\n')
                && (preferred_break_after(before) || script_boundary(before, after))
        })
        .filter_map(|index| u32::try_from(index).ok())
        .collect()
}

/// Extra hold after a revealed grapheme, in multiples of one grapheme's
/// reveal time: a reader's voice rests a little at a comma and longer at the
/// end of a sentence. Returns 0 for any other grapheme.
#[must_use]
pub fn reveal_rest_after(grapheme: &str) -> u32 {
    match grapheme.chars().last() {
        Some('、' | '，' | ',') => 3,
        Some('。' | '．' | '！' | '？' | '!' | '?') => 6,
        Some('…' | '‥' | '—' | '―') => 2,
        _ => 0,
    }
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

fn choose_line_break(graphemes: &[&str], start: usize, fitted_end: usize, fill: LineFill) -> usize {
    // On a book's page a comma or full stop that would open the next line
    // hangs past the measure instead (burasage), as a printed page sets it,
    // rather than taking the last character down with it.
    if fill == LineFill::Full
        && graphemes
            .get(fitted_end)
            .is_some_and(|mark| matches!(*mark, "、" | "。" | "，" | "．"))
        && graphemes
            .get(fitted_end + 1)
            .is_none_or(|next| !starts_with_forbidden(next, FORBIDDEN_LINE_START))
    {
        return fitted_end + 1;
    }
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

    if fill == LineFill::Full {
        return maximum;
    }

    let allowed = |candidate: usize| {
        candidate == graphemes.len()
            || (!starts_with_forbidden(graphemes[candidate], FORBIDDEN_LINE_START)
                && !starts_with_forbidden(graphemes[candidate - 1], FORBIDDEN_LINE_END))
    };

    // When the rest of the sentence is only a scrap (「…になってし」/「まった。」),
    // a full first line leaves an orphan that reads as a broken word. Balance
    // the two lines instead: break at the phrase edge nearest the middle, or
    // failing that at a change of script nearest the middle.
    let rest = graphemes.len() - maximum;
    let fitted = maximum - start;
    if rest > 0 && rest * 3 <= fitted && rest <= fitted {
        let middle = start + (graphemes.len() - start) / 2;
        let lower = (start + (graphemes.len() - start) / 3).max(minimum);
        let nearest = |accept: &dyn Fn(usize) -> bool| {
            (lower..=maximum)
                .filter(|&candidate| allowed(candidate) && accept(candidate))
                .min_by_key(|&candidate| candidate.abs_diff(middle))
        };
        if let Some(candidate) = nearest(&|c| preferred_break_after(graphemes[c - 1])) {
            return candidate;
        }
        if let Some(candidate) = nearest(&|c| script_boundary(graphemes[c - 1], graphemes[c])) {
            return candidate;
        }
    }

    // Prefer a natural sentence/phrase edge near the available width.  The
    // short search window prevents every comma in a long line from producing
    // a ragged, unnecessarily narrow subtitle.
    let preferred_start = maximum.saturating_sub(8).max(minimum);
    for candidate in (preferred_start..=maximum).rev() {
        if preferred_break_after(graphemes[candidate - 1]) && allowed(candidate) {
            return candidate;
        }
    }
    // With no phrase edge in reach, a change from kana to kanji or katakana is
    // the likeliest word boundary; it is better than splitting a word.
    for candidate in (preferred_start..=maximum).rev() {
        if candidate < graphemes.len()
            && script_boundary(graphemes[candidate - 1], graphemes[candidate])
            && allowed(candidate)
        {
            return candidate;
        }
    }
    maximum
}

/// A likely word boundary between two graphemes: kana followed by kanji,
/// katakana, or an opening bracket, where a new phrase usually begins.
fn script_boundary(before: &str, after: &str) -> bool {
    let kind = |grapheme: &str| grapheme.chars().next().map(script_of);
    matches!(
        (kind(before), kind(after)),
        (
            Some(Script::Hiragana),
            Some(Script::Kanji | Script::Katakana | Script::Opening)
        )
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Hiragana,
    Katakana,
    Kanji,
    Opening,
    Other,
}

fn script_of(character: char) -> Script {
    match character {
        '\u{3041}'..='\u{309F}' => Script::Hiragana,
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' => Script::Katakana,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' => Script::Kanji,
        character if FORBIDDEN_LINE_END.contains(character) => Script::Opening,
        _ => Script::Other,
    }
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
    fn statement_breaks_fall_only_at_phrase_edges() {
        let text = "「だって、人が死ぬところなんて……わざわざ見たい人なんて、いないもんね」";
        let graphemes = UnicodeSegmentation::graphemes(text, true).collect::<Vec<_>>();
        let breaks = phrase_break_opportunities(text);
        let at = |index: u32| graphemes[..index as usize].concat();
        // After the comma and between 「人が」 and 「死ぬ」 ...
        assert!(
            breaks.iter().any(|&b| at(b).ends_with("だって、")),
            "{breaks:?}"
        );
        assert!(
            breaks.iter().any(|&b| at(b).ends_with("人が")),
            "{breaks:?}"
        );
        // ... but never inside 「なんて」, and never before a closing mark.
        assert!(
            !breaks.iter().any(|&b| at(b).ends_with("人な")),
            "{breaks:?}"
        );
        assert!(
            !breaks
                .iter()
                .any(|&b| graphemes[b as usize].starts_with('」')),
            "{breaks:?}"
        );
        assert!(
            !breaks.iter().any(|&b| graphemes[b as usize] == "…"),
            "{breaks:?}"
        );
    }

    #[test]
    fn a_paragraph_that_fits_a_page_is_not_split_across_two() {
        // Three paragraphs of one, two and one lines on two-line pages: the
        // second does not fit after the first, so it opens the next page.
        let text = "あいう\nかきくけこさしすせそ\nたち";
        let pages = paginate_subtitles(text, 12, 2);
        assert_eq!(pages, vec!["あいう", "かきくけこさ\nしすせそ", "たち"]);
        // On a page of six lines, all of it is one page.
        assert_eq!(paginate_subtitles(text, 12, 6).len(), 1);
        // A paragraph longer than a page is still cut.
        assert_eq!(
            paginate_subtitles("あいうえおかきくけこさしすせそ", 6, 2).len(),
            3
        );
    }

    #[test]
    fn a_book_page_fills_each_line_to_the_measure() {
        let text = "シャッターの閉まった売店。酔っ払いが残していったゲロと酒の混ざり合った匂い。";
        // The band balances the scrap at the sentence end ...
        let band = paginate_subtitles(text, 64, 2);
        assert_eq!(
            band[0].split('\n').next(),
            Some("シャッターの閉まった売店。")
        );
        // ... a page fills the first line and keeps 「。」 off a line start.
        let page = paginate_subtitles(text, 64, 6);
        let lines = page[0].split('\n').collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(UnicodeSegmentation::graphemes(lines[0], true).count(), 32);
        assert!(!lines[1].starts_with('。'));
        // A full stop that would open a line hangs at the end of the last.
        let hung = paginate_subtitles("あいうえお。かき", 10, 6);
        assert_eq!(hung, vec!["あいうえお。\nかき"]);
    }

    #[test]
    fn a_reader_rests_at_commas_and_sentence_ends() {
        assert_eq!(reveal_rest_after("、"), 3);
        assert_eq!(reveal_rest_after("。"), 6);
        assert_eq!(reveal_rest_after("？"), 6);
        assert_eq!(reveal_rest_after("あ"), 0);
        assert_eq!(reveal_rest_after("」"), 0);
    }

    #[test]
    fn a_scrap_of_a_sentence_is_balanced_instead_of_orphaned() {
        // 53 graphemes in a 48-wide band: a full first line would leave
        // 「まった。」 alone and split 「しまった」 across the break.
        let lines = wrap_japanese(
            "小学校の時に初めて倒れて入院して以来、私の世界は、この一部屋に収まるほどのちんけなものになってしまった。",
            96,
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with('、'), "{lines:?}");
        assert!(lines[1].chars().count() >= 20, "{lines:?}");
    }

    #[test]
    fn without_punctuation_a_line_breaks_where_kana_meets_kanji() {
        let lines = wrap_japanese("ああああああああああああああ電車がくる", 30);
        assert!(lines[0].ends_with('あ'), "{lines:?}");
        assert!(lines[1].starts_with('電'), "{lines:?}");
    }

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
