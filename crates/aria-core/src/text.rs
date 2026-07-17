use unicode_segmentation::UnicodeSegmentation;

const FORBIDDEN_LINE_START: &str =
    "、。，．・：；？！ー〜…‥ヽヾゝゞ々〻）］｝〕〉》」』】〙〗〟’”｠»";
const FORBIDDEN_LINE_END: &str = "（［｛〔〈《「『【〘〖〝‘“｟«";

/// Wraps text without splitting grapheme clusters and applies basic Japanese
/// line-start/line-end prohibition rules. Width is measured in display cells:
/// ASCII uses one cell and non-ASCII graphemes use two.
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

            while end < graphemes.len()
                && starts_with_forbidden(graphemes[end], FORBIDDEN_LINE_START)
            {
                end += 1;
            }
            while end > start + 1 && starts_with_forbidden(graphemes[end - 1], FORBIDDEN_LINE_END) {
                end -= 1;
            }
            lines.push(graphemes[start..end].concat());
            start = end;
        }
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
}
