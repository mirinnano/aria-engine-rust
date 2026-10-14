//! In-line text markup, in the notation Japanese web-novel authors already use.
//!
//! - Ruby: `|漢字《かんじ》` (or `｜`), or `漢字《かんじ》` where the base is the
//!   run of kanji immediately before `《`.
//! - Emphasis dots (傍点): `《《ここ》》`.
//! - A pause inside a line: `{wait:300}` (milliseconds; `ms` is optional).
//!
//! Text that does not complete one of these forms stays literal, so ordinary
//! prose never needs escaping. Positions are grapheme indices into the plain
//! text, which is what history, search, and line keys see.

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarkupKind {
    Ruby { text: String },
    Emphasis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkupSpan {
    /// Grapheme range `[start, end)` in the text the span is reported for.
    pub start: usize,
    pub end: usize,
    #[serde(flatten)]
    pub kind: MarkupKind,
}

/// A pause before the grapheme at `at` is revealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextWait {
    pub at: usize,
    pub ms: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedText {
    pub plain: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<MarkupSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waits: Vec<TextWait>,
}

impl ParsedText {
    #[must_use]
    pub fn has_markup(&self) -> bool {
        !self.spans.is_empty() || !self.waits.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MarkupError {
    #[error("ruby '《{0}》' has no base text; write '|base《{0}》'")]
    RubyWithoutBase(String),
    #[error("empty ruby after '{0}'")]
    EmptyRuby(String),
    #[error("'{{{0}}}' is not a text command; use '{{wait:300}}'")]
    UnknownCommand(String),
}

fn is_kanji(grapheme: &str) -> bool {
    grapheme.chars().all(|character| {
        matches!(character,
            '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{2FA1F}' | '々' | '〆' | 'ヶ' | '〇')
    })
}

/// Parses authored text into plain text plus markup.
pub fn parse(source: &str) -> Result<ParsedText, MarkupError> {
    let graphemes: Vec<&str> = source.graphemes(true).collect();
    let mut plain: Vec<String> = Vec::new();
    let mut spans = Vec::new();
    let mut waits = Vec::new();
    // Where an explicit `|` ruby base begins in `plain`.
    let mut bar_base: Option<usize> = None;
    let mut index = 0;
    let find = |from: usize, target: &str| {
        graphemes[from..]
            .iter()
            .position(|g| *g == target)
            .map(|offset| from + offset)
    };

    while index < graphemes.len() {
        let grapheme = graphemes[index];
        // Emphasis: 《《text》》
        if grapheme == "《"
            && graphemes.get(index + 1) == Some(&"《")
            && let Some(close) = (index + 2..graphemes.len().saturating_sub(1))
                .find(|at| graphemes[*at] == "》" && graphemes[*at + 1] == "》")
        {
            let start = plain.len();
            plain.extend(graphemes[index + 2..close].iter().map(|g| (*g).to_owned()));
            spans.push(MarkupSpan {
                start,
                end: plain.len(),
                kind: MarkupKind::Emphasis,
            });
            index = close + 2;
            continue;
        }
        // Ruby: base《ruby》
        if grapheme == "《"
            && let Some(close) = find(index + 1, "》")
        {
            let ruby: String = graphemes[index + 1..close].concat();
            let start = match bar_base.take() {
                Some(start) => start,
                None => {
                    let mut start = plain.len();
                    while start > 0 && is_kanji(&plain[start - 1]) {
                        start -= 1;
                    }
                    start
                }
            };
            if start == plain.len() {
                return Err(MarkupError::RubyWithoutBase(ruby));
            }
            if ruby.is_empty() {
                return Err(MarkupError::EmptyRuby(plain[start..].concat()));
            }
            spans.push(MarkupSpan {
                start,
                end: plain.len(),
                kind: MarkupKind::Ruby { text: ruby },
            });
            index = close + 1;
            continue;
        }
        // Explicit ruby base marker, only when a 《…》 follows on this line.
        if (grapheme == "|" || grapheme == "｜")
            && let Some(open) = find(index + 1, "《")
            && open > index + 1
            && find(open + 1, "》").is_some()
            && !graphemes[index + 1..open].contains(&"\n")
        {
            bar_base = Some(plain.len());
            index += 1;
            continue;
        }
        // Commands: {wait:300}
        if grapheme == "{"
            && let Some(close) = find(index + 1, "}")
        {
            let command: String = graphemes[index + 1..close].concat();
            let ms = command
                .strip_prefix("wait:")
                .map(|value| value.trim().trim_end_matches("ms").trim())
                .and_then(|value| value.parse::<u32>().ok());
            match ms {
                Some(ms) => waits.push(TextWait {
                    at: plain.len(),
                    ms,
                }),
                None => return Err(MarkupError::UnknownCommand(command)),
            }
            index = close + 1;
            continue;
        }
        plain.push(grapheme.to_owned());
        index += 1;
    }

    Ok(ParsedText {
        plain: plain.concat(),
        spans,
        waits,
    })
}

/// Re-expresses plain-text markup against one subtitle page. Pages are the
/// plain text with line breaks inserted by the wrapper; a break between pages
/// is dropped. Spans are clipped to the page; positions become page-relative.
#[must_use]
pub fn for_page(parsed: &ParsedText, pages: &[String], page_index: usize) -> ParsedText {
    let plain: Vec<&str> = parsed.plain.graphemes(true).collect();
    let mut cursor = 0;
    for (index, page) in pages.iter().enumerate() {
        // page-relative index of each plain grapheme that lands on this page
        let mut mapping: Vec<(usize, usize)> = Vec::new();
        for (page_at, grapheme) in page.graphemes(true).enumerate() {
            if plain.get(cursor) == Some(&grapheme) {
                mapping.push((cursor, page_at));
                cursor += 1;
            } else if grapheme != "\n" {
                // Unexpected divergence; stop mapping rather than misplace.
                break;
            }
        }
        if index == page_index {
            let to_page = |at: usize| {
                mapping
                    .iter()
                    .find(|(plain_at, _)| *plain_at == at)
                    .map(|(_, page_at)| *page_at)
            };
            let first = mapping
                .first()
                .map(|(plain_at, _)| *plain_at)
                .unwrap_or(cursor);
            let spans = parsed
                .spans
                .iter()
                .filter_map(|span| {
                    let start = span.start.max(first);
                    let end = span.end.min(cursor);
                    (start < end).then(|| MarkupSpan {
                        start: to_page(start).unwrap_or(0),
                        end: to_page(end - 1).map_or(0, |at| at + 1),
                        kind: span.kind.clone(),
                    })
                })
                .collect();
            let waits = parsed
                .waits
                .iter()
                .filter(|wait| wait.at >= first && wait.at < cursor)
                .filter_map(|wait| to_page(wait.at).map(|at| TextWait { at, ms: wait.ms }))
                .collect();
            return ParsedText {
                plain: page.clone(),
                spans,
                waits,
            };
        }
        // An original newline at a page boundary is not on either page.
        if plain.get(cursor) == Some(&"\n") {
            cursor += 1;
        }
    }
    ParsedText::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruby_with_bar_and_implicit_kanji_base() {
        let parsed = parse("|宍道湖《しんじこ》へ向かう。病室《びょうしつ》").unwrap();
        assert_eq!(parsed.plain, "宍道湖へ向かう。病室");
        assert_eq!(
            parsed.spans,
            vec![
                MarkupSpan {
                    start: 0,
                    end: 3,
                    kind: MarkupKind::Ruby {
                        text: "しんじこ".into()
                    }
                },
                MarkupSpan {
                    start: 8,
                    end: 10,
                    kind: MarkupKind::Ruby {
                        text: "びょうしつ".into()
                    }
                },
            ]
        );
    }

    #[test]
    fn emphasis_and_wait() {
        let parsed = parse("それは《《ここ》》だった。{wait:400}ほんとうに。").unwrap();
        assert_eq!(parsed.plain, "それはここだった。ほんとうに。");
        assert_eq!(
            parsed.spans[0],
            MarkupSpan {
                start: 3,
                end: 5,
                kind: MarkupKind::Emphasis
            }
        );
        assert_eq!(parsed.waits, vec![TextWait { at: 9, ms: 400 }]);
    }

    #[test]
    fn unmatched_marks_stay_literal() {
        let parsed = parse("「|」と『{』だけ。").unwrap();
        assert_eq!(parsed.plain, "「|」と『{』だけ。");
        assert!(!parsed.has_markup());
    }

    #[test]
    fn errors_name_the_problem() {
        assert!(matches!(
            parse("あ《い》"),
            Err(MarkupError::RubyWithoutBase(_))
        ));
        assert!(matches!(
            parse("{pause}"),
            Err(MarkupError::UnknownCommand(_))
        ));
    }

    #[test]
    fn page_mapping_skips_inserted_breaks() {
        let parsed = parse("あいう|漢字《かんじ》えお").unwrap();
        let pages = vec!["あいう\n漢字".to_owned(), "えお".to_owned()];
        let first = for_page(&parsed, &pages, 0);
        assert_eq!(first.spans[0].start, 4);
        assert_eq!(first.spans[0].end, 6);
        assert!(for_page(&parsed, &pages, 1).spans.is_empty());
    }
}
