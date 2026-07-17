use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxTree {
    source_id: String,
    source: String,
    pub lines: Vec<SyntaxLine>,
    pub diagnostics: Vec<Diagnostic>,
}

impl SyntaxTree {
    #[must_use]
    pub fn parse(source_id: impl Into<String>, source: impl Into<String>) -> Self {
        let source_id = source_id.into();
        let source = source.into();
        let mut lines = Vec::new();
        let mut diagnostics = Vec::new();

        for (index, raw) in source.split_inclusive('\n').enumerate() {
            let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let without_newline = raw.strip_suffix('\n').unwrap_or(raw);
            let without_newline = without_newline
                .strip_suffix('\r')
                .unwrap_or(without_newline);
            let trimmed = without_newline.trim();
            let span = SourceSpan::line(&source_id, line_number, without_newline.len());
            let kind = classify_line(trimmed, &span, &mut diagnostics);
            lines.push(SyntaxLine {
                line: line_number,
                raw: raw.to_owned(),
                kind,
            });
        }

        if source.is_empty() {
            lines.clear();
        } else if !source.ends_with('\n') && lines.is_empty() {
            let span = SourceSpan::line(&source_id, 1, source.len());
            let kind = classify_line(source.trim(), &span, &mut diagnostics);
            lines.push(SyntaxLine {
                line: 1,
                raw: source.clone(),
                kind,
            });
        }

        Self {
            source_id,
            source,
            lines,
            diagnostics,
        }
    }

    #[must_use]
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    /// Returns the input byte-for-byte, including original whitespace and EOLs.
    #[must_use]
    pub fn lossless_source(&self) -> &str {
        &self.source
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxLine {
    pub line: u32,
    pub raw: String,
    pub kind: SyntaxKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxKind {
    Empty,
    Comment,
    Directive {
        name: String,
        value: String,
    },
    Label(String),
    Assignment {
        target: String,
        operator: String,
        value: String,
    },
    Command(CommandSyntax),
    Dialogue {
        speaker: Option<String>,
        content: String,
    },
    Advance {
        clear_page: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSyntax {
    pub name: String,
    pub arguments: Vec<String>,
    pub raw_arguments: String,
}

fn classify_line(
    trimmed: &str,
    span: &SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> SyntaxKind {
    if trimmed.is_empty() {
        return SyntaxKind::Empty;
    }
    if trimmed.starts_with(';') || trimmed.starts_with("//") {
        return SyntaxKind::Comment;
    }
    if let Some(directive) = trimmed.strip_prefix('#') {
        let (name, value) = directive
            .split_once(':')
            .or_else(|| directive.split_once(char::is_whitespace))
            .unwrap_or((directive, ""));
        return SyntaxKind::Directive {
            name: name.trim().to_ascii_lowercase(),
            value: value.trim().to_owned(),
        };
    }
    if trimmed == "@" {
        return SyntaxKind::Advance { clear_page: false };
    }
    if trimmed == "\\" {
        return SyntaxKind::Advance { clear_page: true };
    }
    if let Some(label) = trimmed.strip_prefix('*')
        && (label.is_empty()
            || label
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
    {
        let label = label.trim();
        if label.is_empty() || !label.bytes().all(valid_identifier_byte) {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidSyntax,
                format!("invalid label '{label}'"),
                Some(span.clone()),
            ));
        }
        return SyntaxKind::Label(label.to_owned());
    }

    let name_end = trimmed
        .find(|character: char| character.is_whitespace() || character == '(')
        .unwrap_or(trimmed.len());
    let name = &trimmed[..name_end];

    // Assignment and block syntax must win over the permissive bare-text
    // fallback.  The legacy compiler still lowers these through its register
    // command bridge, but classifying them here prevents a typo such as
    // `%route = 1` from silently becoming dialogue.
    if let Some((target, operator, value)) = parse_assignment(trimmed) {
        return SyntaxKind::Assignment {
            target,
            operator,
            value,
        };
    }

    // Japanese dialogue is intentionally checked before the generic command
    // path, except for a reserved command name.  This makes
    // `ミオ「本文」` and `「地の文」` deterministic while keeping a malformed
    // `say ...` line in the command grammar where it can receive an operand
    // diagnostic.
    if let Some(open) = trimmed.find('「') {
        if let Some(close) = trimmed.rfind('」')
            && close > open
            && !is_reserved_command(name)
        {
            let speaker = trimmed[..open].trim();
            let content = &trimmed[open + '「'.len_utf8()..close];
            return SyntaxKind::Dialogue {
                speaker: (!speaker.is_empty()).then(|| speaker.to_owned()),
                content: content.to_owned(),
            };
        }
        if !trimmed.contains('」') {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidSyntax,
                "unterminated Japanese dialogue; expected '」'",
                Some(span.clone()),
            ));
        }
    }

    if name.bytes().next().is_some_and(valid_command_start)
        && name.bytes().all(valid_identifier_byte)
    {
        let mut arguments = trimmed[name_end..].trim();
        if arguments.starts_with('(') && arguments.ends_with(')') {
            arguments = &arguments[1..arguments.len() - 1];
        }
        let arguments_without_comment = strip_inline_comment(arguments);
        let parsed = split_arguments(arguments_without_comment);
        if !is_reserved_command(name) && looks_like_command_arguments(arguments_without_comment) {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnknownCommand,
                format!(
                    "unknown command '{}'; quote the line or use a supported Aria command",
                    name
                ),
                Some(span.clone()),
            ));
        }
        match parsed {
            Ok(parsed) => SyntaxKind::Command(CommandSyntax {
                name: name.to_ascii_lowercase(),
                arguments: parsed,
                raw_arguments: arguments_without_comment.trim().to_owned(),
            }),
            Err(message) => {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidSyntax,
                    message,
                    Some(span.clone()),
                ));
                SyntaxKind::Command(CommandSyntax {
                    name: name.to_ascii_lowercase(),
                    arguments: Vec::new(),
                    raw_arguments: arguments_without_comment.trim().to_owned(),
                })
            }
        }
    } else if let Some(open) = trimmed.find('「')
        && let Some(close) = trimmed.rfind('」')
        && close > open
    {
        let speaker = trimmed[..open].trim();
        let content = &trimmed[open + '「'.len_utf8()..close];
        SyntaxKind::Dialogue {
            speaker: (!speaker.is_empty()).then(|| speaker.to_owned()),
            content: content.to_owned(),
        }
    } else {
        SyntaxKind::Dialogue {
            speaker: None,
            content: trimmed.to_owned(),
        }
    }
}

fn parse_assignment(input: &str) -> Option<(String, String, String)> {
    let mut operator = None;
    let mut operator_start = 0;
    for (index, character) in input.char_indices() {
        if matches!(character, '=' | '+') {
            if character == '+' && input[index + character.len_utf8()..].starts_with('=') {
                operator = Some("+=");
                operator_start = index;
                break;
            }
            if character == '=' {
                operator = Some("=");
                operator_start = index;
                break;
            }
        }
    }
    let operator = operator?;
    let target = input[..operator_start].trim();
    let value_start = operator_start + operator.len();
    let value = input[value_start..].trim();
    if target.is_empty() || value.is_empty() || !valid_assignment_target(target) {
        return None;
    }
    Some((target.to_owned(), operator.to_owned(), value.to_owned()))
}

fn valid_assignment_target(target: &str) -> bool {
    target.starts_with('%')
        || target.starts_with('$')
        || (target.bytes().next().is_some_and(valid_command_start)
            && target.bytes().all(valid_identifier_byte))
}

fn looks_like_command_arguments(arguments: &str) -> bool {
    let trimmed = arguments.trim();
    trimmed.starts_with('(')
        || trimmed.starts_with('=')
        || trimmed.starts_with('{')
        || trimmed.starts_with('}')
        || trimmed.contains(',')
        || trimmed.split_whitespace().next().is_some_and(|first| {
            first.starts_with('"')
                || first.starts_with('\'')
                || first.starts_with('%')
                || first.starts_with('$')
                || first.parse::<i64>().is_ok()
        })
}

fn is_reserved_command(name: &str) -> bool {
    const COMMANDS: &[&str] = &[
        "include",
        "use",
        "strict",
        "compat_mode",
        "debug",
        "caption",
        "window",
        "font",
        "font_atlas_size",
        "font_filter",
        "script",
        "func",
        "endfunc",
        "defsub",
        "getparam",
        "goto",
        "jmp",
        "gosub",
        "return",
        "if",
        "else",
        "endif",
        "while",
        "wend",
        "text",
        "say",
        "narrate",
        "await",
        "advance",
        "textclear",
        "erasetextwindow",
        "waitclick",
        "wait_click",
        "wait",
        "bg",
        "loadbg",
        "load_bg",
        "transition",
        "lsp",
        "loadch",
        "load_ch",
        "lsp_text",
        "ui_text",
        "ui",
        "lsp_rect",
        "ui_rect",
        "csp",
        "clr",
        "hidech",
        "hide_ch",
        "vsp",
        "showch",
        "show_ch",
        "msp",
        "charmove",
        "char_move",
        "choice",
        "let",
        "mov",
        "add",
        "sub",
        "inc",
        "dec",
        "playbgm",
        "play_bgm",
        "bgm",
        "playmp3",
        "dwave",
        "playse",
        "play_se",
        "dwaveloop",
        "voice",
        "stopbgm",
        "stop_bgm",
        "mp3fadeout",
        "dwavestop",
        "stopse",
        "stop_se",
        "voice_stop",
        "voicestop",
        "bgmvol",
        "bgm_vol",
        "sevol",
        "se_vol",
        "save",
        "load",
        "end",
        "quit",
    ];
    COMMANDS
        .iter()
        .any(|command| command.eq_ignore_ascii_case(name))
}

fn valid_command_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn valid_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}

#[must_use]
pub fn strip_inline_comment(input: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if character == ';' && quote.is_none() {
            return &input[..index];
        }
    }
    input
}

pub fn split_arguments(input: &str) -> Result<Vec<String>, String> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_u32;
    for (index, character) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if quote.is_some() {
            continue;
        }
        match character {
            '(' | '[' | '{' => depth = depth.saturating_add(1),
            ')' | ']' | '}' => {
                if depth == 0 {
                    return Err("unmatched closing delimiter".to_owned());
                }
                depth -= 1;
            }
            ',' if depth == 0 => {
                let argument = input[start..index].trim();
                if argument.is_empty() {
                    return Err("empty argument".to_owned());
                }
                arguments.push(argument.to_owned());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return Err("unterminated string literal".to_owned());
    }
    if depth != 0 {
        return Err("unterminated delimiter".to_owned());
    }
    let tail = input[start..].trim();
    if tail.is_empty() {
        return Err("trailing comma creates an empty argument".to_owned());
    }
    arguments.push(tail.to_owned());
    Ok(arguments)
}

#[must_use]
pub fn unquote(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            let inner = &trimmed[1..trimmed.len() - 1];
            let mut output = String::with_capacity(inner.len());
            let mut escaped = false;
            for character in inner.chars() {
                if escaped {
                    output.push(match character {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => other,
                    });
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else {
                    output.push(character);
                }
            }
            if escaped {
                output.push('\\');
            }
            return output;
        }
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_tree_is_lossless_and_understands_japanese_dialogue() {
        let source = "# aria-version: 3.0\r\n; comment\nミオ「海へ行こう。」\n@";
        let tree = SyntaxTree::parse("main.aria", source);
        assert_eq!(tree.lossless_source(), source);
        assert!(matches!(
            &tree.lines[2].kind,
            SyntaxKind::Dialogue { speaker: Some(value), content }
                if value == "ミオ" && content == "海へ行こう。"
        ));
        assert!(tree.diagnostics.is_empty());
    }

    #[test]
    fn arguments_keep_commas_inside_strings_and_calls() {
        let values = split_arguments("1, \"hello, world\", rgba(1, 2, 3, 4)").unwrap();
        assert_eq!(values, ["1", "\"hello, world\"", "rgba(1, 2, 3, 4)"]);
    }

    #[test]
    fn markdown_bold_at_line_start_is_dialogue_not_a_label() {
        let tree = SyntaxTree::parse("main.aria", "**９月２１日（金曜日）**\\\n");
        assert!(tree.diagnostics.is_empty());
        assert!(matches!(tree.lines[0].kind, SyntaxKind::Dialogue { .. }));
    }

    #[test]
    fn bare_lines_and_japanese_dialogue_are_display_syntax() {
        let tree = SyntaxTree::parse(
            "main.aria",
            "ミオ「本文」\n「地の文」\nこれは引用符のない本文です\n@\n",
        );
        assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
        assert!(matches!(
            tree.lines[0].kind,
            SyntaxKind::Dialogue { speaker: Some(ref speaker), ref content }
                if speaker == "ミオ" && content == "本文"
        ));
        assert!(matches!(
            tree.lines[1].kind,
            SyntaxKind::Dialogue { speaker: None, ref content } if content == "地の文"
        ));
        assert!(matches!(tree.lines[2].kind, SyntaxKind::Dialogue { .. }));
        assert!(matches!(
            tree.lines[3].kind,
            SyntaxKind::Advance { clear_page: false }
        ));
    }

    #[test]
    fn reserved_commands_win_and_unknown_command_like_lines_error() {
        let tree = SyntaxTree::parse(
            "main.aria",
            "say \"本文\"\nawait advance\nmystery 1, 2\nscore = 1\n",
        );
        assert!(matches!(tree.lines[0].kind, SyntaxKind::Command(_)));
        assert!(matches!(tree.lines[1].kind, SyntaxKind::Command(_)));
        assert!(matches!(tree.lines[3].kind, SyntaxKind::Assignment { .. }));
        assert!(
            tree.diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == DiagnosticCode::UnknownCommand })
        );
    }
}
