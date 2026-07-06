use std::borrow::Cow;

use winnow::{
    combinator::{alt, delimited, fail, opt, peek, preceded, repeat, trace},
    error::{StrContext, StrContextValue},
    stream::AsChar,
    token::{any, literal, one_of, take_while},
    Parser,
};

use super::{
    chars,
    context::{self, FlowOrKey},
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// Double quoted text.
///
/// https://yaml.org/spec/1.2.2/#rule-c-double-quoted
#[doc(alias = "c-double-quoted")]
pub fn double_quoted<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "double::double_quoted",
        delimited(
            one_of('"'),
            non_break_double_text(context, indent_level),
            one_of('"'),
        ),
    )
}

/// Double quoted text content.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-double-text
#[doc(alias = "nb-double-text")]
pub(super) fn non_break_double_text<'i, Context, Input, Error>(
    _context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "double::non_break_double_text",
        Context::non_break_double_text(indent_level),
    )
}

/// Double quoted text content, multi lines.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-double-multi-line
#[doc(alias = "nb-double-multi-line")]
pub(super) fn non_break_double_multi_line<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "double::non_break_double_multi_line",
        move |input: &mut Input| {
            let (mut current, mut literal_tail_len) = non_break_double_chars.parse_next(input)?;
            loop {
                let may_break = opt(alt((
                    double_escaped_line_break(indent_level).map(|s| (false, s)),
                    spaces::flow_folded(indent_level).map(|s| (true, s)),
                )))
                .parse_next(input)?;
                let (must_trim, line_break) = match may_break {
                    None => return Ok(current),
                    Some((must_trim, line_break)) => (must_trim, line_break),
                };
                if must_trim {
                    // Only the trailing run of characters copied verbatim from the
                    // source (not produced by an escape sequence) may be trimmed:
                    // an escaped space/tab (e.g. "\t" or "\ ") must survive folding
                    // even though it resolves to a whitespace character.
                    let tail_start = current.len() - literal_tail_len;
                    let tail = &current[tail_start..];
                    let ws_trim = tail.len() - tail.trim_end_matches(chars::is_white_space).len();
                    let newlen = current.len() - ws_trim;
                    current.to_mut().truncate(newlen);
                }
                current.to_mut().push_str(&line_break);
                let (next_line, next_literal_tail_len) =
                    non_break_double_chars.parse_next(input)?;
                current.to_mut().push_str(&next_line);
                literal_tail_len = next_literal_tail_len;
            }
        },
    )
}

/// Double quoted text escaped break.
///
/// https://yaml.org/spec/1.2.2/#rule-s-double-escaped
#[doc(alias = "s-double-escaped")]
fn double_escaped_line_break<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "double::double_escaped_line_break",
        move |input: &mut Input| {
            let count: usize = delimited(
                literal("\\\n"),
                repeat(0.., spaces::line_empty(context::FlowIn, indent_level)),
                spaces::flow_line_prefix(indent_level),
            )
            .parse_next(input)?;
            if count > 0 {
                Ok(Cow::Owned("\n".repeat(count)))
            } else {
                Ok(Cow::Borrowed(""))
            }
        },
    )
}

/// Double quoted text content, one line.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-double-one-line
#[doc(alias = "nb-double-one-line")]
pub(super) fn non_break_double_one_line<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "double::non_break_double_one_line",
        non_break_double_chars.map(|(s, _)| s),
    )
    .parse_next(input)
}

/// Double quoted chars.
///
/// Returns the resolved content together with the byte length of its
/// trailing run of characters that were copied verbatim from the source
/// (i.e. not produced by an escape sequence). Callers that fold line breaks
/// use that length to avoid trimming whitespace that only exists because an
/// escape sequence (e.g. `\t`, `\ `) resolved to a whitespace character.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-double-char
#[doc(alias = "nb-double-char")]
fn non_break_double_chars<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<(Cow<'i, str>, usize), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "double::non_break_double_chars",
        move |input: &mut Input| {
            let initial = take_while(0.., is_normal_double_char).parse_next(input)?;
            let mut current: Cow<str> = Cow::Borrowed(initial);
            let mut literal_tail_len = initial.len();
            loop {
                let escaped_newline = peek(opt(literal("\\\n"))).parse_next(input)?;
                if escaped_newline.is_some() {
                    // escaped newline is handled outside of this function.
                    return Ok((current, literal_tail_len));
                }
                let maybe_escape = opt(preceded(one_of('\\'), any)).parse_next(input)?;
                let esc: char = match maybe_escape {
                    None => return Ok((current, literal_tail_len)),
                    Some(c) => c,
                };
                let c = match (esc, escape_to_char(esc)) {
                    (_, Some(c)) => Ok(c),
                    ('x', _) => take_while(2, AsChar::is_hex_digit)
                        .try_map(|s: &str| u8::from_str_radix(s, 16))
                        .verify_map(|c: u8| char::from_u32(c.into()))
                        .parse_next(input),
                    ('u', _) => take_while(4, AsChar::is_hex_digit)
                        .try_map(|s: &str| u16::from_str_radix(s, 16))
                        .verify_map(|c: u16| char::from_u32(c.into()))
                        .parse_next(input),
                    ('U', _) => take_while(8, AsChar::is_hex_digit)
                        .try_map(|s: &str| u32::from_str_radix(s, 16))
                        .verify_map(|c: u32| char::from_u32(c))
                        .parse_next(input),
                    _ => fail
                        .context(StrContext::Expected(StrContextValue::Description(
                            "YAML supported escape sequence",
                        )))
                        .parse_next(input),
                }?;
                current.to_mut().push(c);
                let next_normal = take_while(0.., is_normal_double_char).parse_next(input)?;
                current.to_mut().push_str(next_normal);
                literal_tail_len = next_normal.len();
            }
        },
    )
    .parse_next(input)
}

/// Returns the original character from escape sequence.
#[inline]
fn escape_to_char(escape: char) -> Option<char> {
    match escape {
        '0' => Some('\x00'),
        'a' => Some('\x07'),
        'b' => Some('\x08'),
        't' | '\t' => Some('\t'),
        'n' => Some('\n'),
        'v' => Some('\x0b'),
        'f' => Some('\x0c'),
        'r' => Some('\r'),
        'e' => Some('\x1b'),
        ' ' => Some(' '),
        '"' => Some('"'),
        '/' => Some('/'),
        '\\' => Some('\\'),
        'N' => Some('\u{85}'),
        '_' => Some('\u{a0}'),
        'L' => Some('\u{2028}'),
        'P' => Some('\u{2029}'),
        _ => None,
    }
}

/// Returns true if the char is safe character in the double scaped chars.
#[inline]
fn is_normal_double_char(c: char) -> bool {
    match c {
        '\\' => false,
        '"' => false,
        '\t' => true,
        _ => chars::is_nb_json(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::testing;

    #[test]
    fn double_quoted_folding() {
        let input = concat!(
            r#""folded "#,
            "\n",
            "to a space,\t\n",
            " \n",
            "to a line feed, or \t",
            r#"\"#,
            "\n",
            r#" \ "#,
            "\tnon-content\""
        );
        assert_eq!(
            (
                "",
                Cow::Owned("folded to a space,\nto a line feed, or \t \tnon-content".to_string())
            ),
            testing::parse(
                double_quoted(context::FlowIn, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }

    #[test]
    fn double_quoted_lines() {
        let input = concat!(
            "\" 1st non-empty\n",
            "\n",
            " 2nd non-empty \n",
            "\t3rd non-empty \""
        );
        assert_eq!(
            (
                "",
                Cow::Owned(" 1st non-empty\n2nd non-empty 3rd non-empty ".to_string())
            ),
            testing::parse(
                double_quoted(context::FlowIn, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }

    #[test]
    fn double_escaped_line_break_parses_input() {
        let got =
            testing::parse(double_escaped_line_break(IndentLevel::initial()), "\\\n").unwrap();
        assert_eq!(("", Cow::Borrowed("")), got);

        let got = testing::parse(
            double_escaped_line_break(IndentLevel::new(3)),
            "\\\n  \n\n    a",
        )
        .unwrap();
        assert_eq!(("a", Cow::Owned("\n\n".to_string())), got);
    }

    #[test]
    fn non_break_double_chars_no_escape() {
        let input = "foo bar\t\x7f\"";
        assert_eq!(
            (
                "\"",
                (Cow::Borrowed("foo bar\t\x7f"), "foo bar\t\x7f".len())
            ),
            testing::parse(non_break_double_chars, input).unwrap()
        );
    }

    #[test]
    fn non_break_double_chars_escape() {
        let input = r#"foo\t\x20bar\u3042いうえお \U00010083\n"#;
        assert_eq!(
            (
                "",
                (Cow::Owned("foo\t barあいうえお \u{10083}\n".to_string()), 0)
            ),
            testing::parse(non_break_double_chars, input).unwrap()
        );
    }

    #[test]
    fn double_quoted_escaped_tab_before_fold_is_preserved() {
        // Corpus case DE56/00: an escaped tab ("\t") immediately followed by
        // an unescaped line break must survive folding.
        let input = "\"1 trailing\\t\n    tab\"";
        assert_eq!(
            ("", Cow::Owned("1 trailing\t tab".to_string())),
            testing::parse(
                double_quoted(context::FlowIn, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }

    #[test]
    fn double_quoted_escaped_tab_with_trailing_spaces_before_fold_is_preserved() {
        // Corpus case DE56/01: trailing unescaped spaces after an escaped tab
        // are trimmed, but the escaped tab itself must survive.
        let input = "\"2 trailing\\t  \n    tab\"";
        assert_eq!(
            ("", Cow::Owned("2 trailing\t tab".to_string())),
            testing::parse(
                double_quoted(context::FlowIn, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }

    #[test]
    fn double_quoted_escaped_raw_tab_before_fold_is_preserved() {
        // Corpus case DE56/02: "\" followed by a literal tab byte is also a
        // valid escape for tab and must survive folding the same way.
        let input = "\"3 trailing\\\t\n    tab\"";
        assert_eq!(
            ("", Cow::Owned("3 trailing\t tab".to_string())),
            testing::parse(
                double_quoted(context::FlowIn, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }

    #[test]
    fn double_quoted_unescaped_trailing_tab_is_trimmed() {
        // Corpus case DE56/04: an unescaped (literal) trailing tab is
        // ordinary trailing whitespace and must be trimmed by folding.
        let input = "\"5 trailing\t\n    tab\"";
        assert_eq!(
            ("", Cow::Owned("5 trailing tab".to_string())),
            testing::parse(
                double_quoted(context::FlowIn, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }
}
