use std::borrow::Cow;

use winnow::{
    combinator::{alt, not, opt, preceded, repeat, separated_foldl1, trace},
    token::{one_of, take_while},
    Parser,
};

use super::header::{self, ChompingMode};
use crate::parse::{
    chars,
    context::BlockIn,
    document,
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// Folded block scalar.
///
/// https://yaml.org/spec/1.2.2/#rule-c-l+folded
#[doc(alias = "c-l+folded")]
pub(super) fn folded<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::folded::folded", move |input: &mut Input| {
        '>'.parse_next(input)?;
        let (indentation, chomping) = header::block_header.parse_next(input)?;
        match indentation {
            Some(m) => folded_content(indent_level + m, chomping).parse_next(input),
            None => {
                // See `detect_indentation`'s doc comment: when it finds no content of its own,
                // the text-matching phase must be skipped entirely rather than attempted at
                // `bound` (which can be a degenerate, always-matching `s-indent(0)`).
                let detected = header::detect_indentation(indent_level).parse_next(input)?;
                match detected.content {
                    Some(content_indent) => {
                        folded_content(content_indent, chomping).parse_next(input)
                    }
                    None => header::chomped_empty(detected.bound, chomping).parse_next(input),
                }
            }
        }
    })
}

/// One folded-style text line: `s-indent(n)` followed by a non-empty run of `nb-char` whose
/// first character is a non-space `ns-char`. That leading-non-space requirement is what
/// distinguishes a foldable line from a "more-indented" ([`spaced_text`]) one, which starts with
/// extra white space instead and is therefore kept literal rather than folded.
///
/// At `indent_level = 0` (only reachable at the document root, spec `n = -1`), `s-indent(0)`
/// matches trivially without consuming anything, and `---`/`...` markers start with non-space
/// characters, so without the [`document::forbidden`] guard this would swallow a marker line as
/// ordinary content. See `document::bare_document`'s doc comment for why `c-forbidden` is
/// checked here rather than once at `l-bare-document`; `spaced_text` doesn't need the same
/// guard since a marker line never starts with whitespace, so it's naturally excluded already.
///
/// https://yaml.org/spec/1.2.2/#rule-s-nb-folded-text
#[doc(alias = "s-nb-folded-text")]
fn folded_text<'i, Input, Error>(indent_level: IndentLevel) -> impl Parser<Input, &'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::folded::folded_text",
        preceded(
            (spaces::indent(indent_level), not(document::forbidden)),
            (
                one_of(chars::is_non_space),
                take_while(0.., chars::is_non_break),
            )
                .take(),
        ),
    )
}

/// Consecutive [`folded_text`] lines, joined by [`spaces::break_line_folded`] (a single break
/// folds to a space, multiple breaks fold to that many minus one line feeds).
///
/// https://yaml.org/spec/1.2.2/#rule-l-nb-folded-lines
#[doc(alias = "l-nb-folded-lines")]
fn folded_lines<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::folded::folded_lines",
        separated_foldl1(
            folded_text(indent_level).map(Cow::Borrowed),
            spaces::break_line_folded(BlockIn, indent_level),
            |mut current: Cow<'i, str>, fold: Cow<'i, str>, text: Cow<'i, str>| {
                current.to_mut().push_str(&fold);
                current.to_mut().push_str(&text);
                current
            },
        ),
    )
}

/// A "more-indented" text line: `s-indent(n)` followed by extra leading white space, then the
/// rest of the line kept verbatim (not folded).
///
/// https://yaml.org/spec/1.2.2/#rule-s-nb-spaced-text
#[doc(alias = "s-nb-spaced-text")]
fn spaced_text<'i, Input, Error>(indent_level: IndentLevel) -> impl Parser<Input, &'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::folded::spaced_text",
        preceded(
            spaces::indent(indent_level),
            (
                one_of(chars::is_white_space),
                take_while(0.., chars::is_non_break),
            )
                .take(),
        ),
    )
}

/// The line break separating two [`spaced_text`] lines, plus any further empty lines -- always
/// literal, never folded.
///
/// https://yaml.org/spec/1.2.2/#rule-b-l-spaced
#[doc(alias = "b-l-spaced")]
fn break_line_spaced<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::folded::break_line_spaced",
        move |input: &mut Input| {
            chars::break_as_line_feed.parse_next(input)?;
            let count: usize =
                repeat(0.., spaces::line_empty(BlockIn, indent_level)).parse_next(input)?;
            Ok(Cow::Owned("\n".repeat(count + 1)))
        },
    )
}

/// Consecutive [`spaced_text`] lines, joined by literal breaks via [`break_line_spaced`].
///
/// https://yaml.org/spec/1.2.2/#rule-l-nb-spaced-lines
#[doc(alias = "l-nb-spaced-lines")]
fn spaced_lines<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::folded::spaced_lines",
        separated_foldl1(
            spaced_text(indent_level).map(Cow::Borrowed),
            break_line_spaced(indent_level),
            |mut current: Cow<'i, str>, br: Cow<'i, str>, text: Cow<'i, str>| {
                current.to_mut().push_str(&br);
                current.to_mut().push_str(&text);
                current
            },
        ),
    )
}

/// Leading empty lines (kept literal) followed by either a foldable or a more-indented run of
/// text lines.
///
/// https://yaml.org/spec/1.2.2/#rule-l-nb-same-lines
#[doc(alias = "l-nb-same-lines")]
fn same_lines<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::folded::same_lines", move |input: &mut Input| {
        let count: usize =
            repeat(0.., spaces::line_empty(BlockIn, indent_level)).parse_next(input)?;
        let rest =
            alt((folded_lines(indent_level), spaced_lines(indent_level))).parse_next(input)?;
        if count > 0 {
            let mut s = "\n".repeat(count);
            s.push_str(&rest);
            Ok(Cow::Owned(s))
        } else {
            Ok(rest)
        }
    })
}

/// Consecutive [`same_lines`] runs, joined by a literal break -- the transition between a
/// foldable run and a more-indented run (or vice versa) is never folded.
///
/// https://yaml.org/spec/1.2.2/#rule-l-nb-diff-lines
#[doc(alias = "l-nb-diff-lines")]
fn diff_lines<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::folded::diff_lines",
        separated_foldl1(
            same_lines(indent_level),
            chars::break_as_line_feed,
            |mut current: Cow<'i, str>, sep: &'static str, text: Cow<'i, str>| {
                current.to_mut().push_str(sep);
                current.to_mut().push_str(&text);
                current
            },
        ),
    )
}

/// Full folded scalar content: the text lines (if any), chomped according to the header's
/// chomping mode.
///
/// https://yaml.org/spec/1.2.2/#rule-l-folded-content
#[doc(alias = "l-folded-content")]
fn folded_content<'i, Input, Error>(
    indent_level: IndentLevel,
    chomping: ChompingMode,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::folded::folded_content", move |input: &mut Input| {
        let mut current: Cow<str> = match opt(diff_lines(indent_level)).parse_next(input)? {
            Some(mut text) => {
                let last = header::chomped_last(chomping).parse_next(input)?;
                if !last.is_empty() {
                    text.to_mut().push_str(last);
                }
                text
            }
            None => Cow::Borrowed(""),
        };
        let trailing = header::chomped_empty(indent_level, chomping).parse_next(input)?;
        if !trailing.is_empty() {
            current.to_mut().push_str(&trailing);
        }
        Ok(current)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::testing;

    /// Spec example 8.8 "Folded Scalar".
    #[test]
    fn folded_scalar_example() {
        assert_eq!(
            ("", Cow::Owned("folded text\n".to_string())),
            testing::parse(folded(IndentLevel::initial()), ">\n folded\n text\n\n").unwrap()
        );
    }

    /// Spec example 8.5 "Chomping Final Line Break", folded half only.
    #[test]
    fn folded_strip_drops_final_break() {
        assert_eq!(
            ("", Cow::Borrowed("text")),
            testing::parse(folded(IndentLevel::new(0)), ">-\n  text\n").unwrap()
        );
    }

    /// Adapted from spec example 8.10 "Folded Lines" (dropping its leading blank line and
    /// "more-indented" bullet-list continuation, which is covered separately by
    /// `folded_more_indented_lines_are_not_folded`): a single break folds to a space, while the
    /// blank line between "line" and "next" (two breaks) folds to a literal line feed instead.
    #[test]
    fn folded_lines_fold_single_breaks_to_spaces() {
        assert_eq!(
            ("", Cow::Owned("folded line\nnext line\n".to_string())),
            testing::parse(
                folded(IndentLevel::initial()),
                ">\n folded\n line\n\n next\n line\n\n"
            )
            .unwrap()
        );
    }

    /// Spec example 8.11 "More Indented Lines": a line starting with extra white space is kept
    /// literal (not folded) and breaks around it stay literal too.
    #[test]
    fn folded_more_indented_lines_are_not_folded() {
        assert_eq!(
            ("", Cow::Owned("literal\n  more\nliteral\n".to_string())),
            testing::parse(
                folded(IndentLevel::initial()),
                ">\n literal\n   more\n literal\n"
            )
            .unwrap()
        );
    }
}
