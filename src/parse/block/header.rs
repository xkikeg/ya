use std::borrow::Cow;

use winnow::{
    combinator::{alt, eof, opt, repeat, trace},
    error::{StrContext, StrContextValue},
    token::{one_of, take_while},
    Parser,
};

use crate::parse::{
    chars,
    context::BlockIn,
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// Chomping mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChompingMode {
    Strip,
    Clip,
    Keep,
}

/// Block chomping indicator.
/// https://yaml.org/spec/1.2.2/#rule-c-chomping-indicator
#[doc(alias = "c-chomping-indicator")]
pub fn chomping_indicator<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<ChompingMode, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::header::chomping_indicator",
        opt(one_of(b"-+")).map(|c| match c {
            Some('-') => ChompingMode::Strip,
            None => ChompingMode::Clip,
            Some('+') => ChompingMode::Keep,
            Some(c) => unreachable!("impossible chomping indicator: {c}"),
        }),
    )
    .parse_next(input)
}

/// Block indentation indicator: a single digit `1`-`9`, giving the content indentation level
/// relative to the block scalar's own indentation level.
///
/// https://yaml.org/spec/1.2.2/#rule-c-indentation-indicator
#[doc(alias = "c-indentation-indicator")]
pub fn indentation_indicator<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<Option<usize>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::header::indentation_indicator",
        opt(one_of('1'..='9')).map(|c: Option<char>| c.map(|c| c as usize - '0' as usize)),
    )
    .parse_next(input)
}

/// Block scalar header: the chomping and indentation indicators, in either order, followed by
/// `s-b-comment`.
///
/// The two indicators use disjoint character sets (`1`-`9` vs. `+`/`-`), so whichever one comes
/// first in the input unambiguously tells us which of the two orders is being used -- no
/// backtracking between the two orders is needed.
///
/// https://yaml.org/spec/1.2.2/#rule-c-b-block-header
#[doc(alias = "c-b-block-header")]
pub fn block_header<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<(Option<usize>, ChompingMode), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::header::block_header", move |input: &mut Input| {
        let indentation_then_chomping = indentation_indicator.parse_next(input)?;
        let (indentation, chomping) = match indentation_then_chomping {
            Some(m) => (Some(m), chomping_indicator.parse_next(input)?),
            None => {
                let chomping = chomping_indicator.parse_next(input)?;
                let indentation = indentation_indicator.parse_next(input)?;
                (indentation, chomping)
            }
        };
        spaces::space_break_comment.parse_next(input)?;
        Ok((indentation, chomping))
    })
    .parse_next(input)
}

/// Detects the content indentation level of a block scalar when no explicit
/// [`indentation_indicator`] was given: the number of leading spaces on the first non-empty
/// line of the contents, or -- if there is no non-empty line at all -- the number of spaces on
/// the longest (most indented) line instead. Both quantities are absolute (not relative to any
/// enclosing indentation level): the spec defines this rule purely as "count the spaces", unlike
/// the indentation-indicator case (`n` + the indicator digit). This deliberately doesn't take an
/// `indent_level` parameter, since well-formed input never needs one here -- see the note below.
///
/// It is an error for a leading empty line to have more spaces than that first non-empty line.
///
/// Implemented by scanning forward line-by-line and resetting the input afterwards, so the real
/// content parse re-consumes normally from the detected `IndentLevel`.
///
/// Outcome of [`detect_indentation`].
#[derive(Debug)]
pub(super) struct DetectedIndentation {
    /// The content's own auto-detected indentation level, when the scalar actually has content
    /// (a non-empty line indented more than the scalar's own `n`). `None` when it doesn't --
    /// callers must then skip the text-line-matching phase entirely (see [`detect_indentation`]'s
    /// doc comment) rather than attempt it at [`Self::bound`].
    pub(super) content: Option<IndentLevel>,
    /// The level to bound trailing/leading blank-line recognition by (`l-chomped-empty` and
    /// friends) regardless of whether [`Self::content`] is present: either that same content
    /// indentation, or -- when there's no content -- the largest indentation seen among any
    /// blank line scanned, so deeply-indented blank lines are still correctly recognized as
    /// blank rather than left unconsumed.
    pub(super) bound: IndentLevel,
}

/// Bounded by the block scalar's own indentation level `n` (the `indent_level` argument, same
/// one given to the enclosing [`literal`](super::literal::literal) /
/// [`folded`](super::folded::folded)): a candidate first non-empty line must be indented *more*
/// than `n`, or it doesn't belong to this scalar's content at all -- it's whatever follows the
/// scalar instead (a sibling node in the enclosing block collection), and the scalar has no
/// content lines of its own ([`DetectedIndentation::content`] is `None`).
///
/// This distinction matters beyond just *which* line counts: when there's no content, callers
/// must not attempt the text-line-matching phase (`literal_text`/`folded_text`) at all, even at
/// [`DetectedIndentation::bound`]. That phase matches via `s-indent(m)` followed by any non-break
/// line, and `s-indent(m)` for `m=0` matches trivially (consuming zero spaces unconditionally) --
/// so at the document root (`n=-1`), where a totally unindented sibling line is completely normal,
/// skipping straight past the text-matching phase is the only way to avoid swallowing that
/// sibling as if it were more scalar content. This surfaced for real once Phase 4 made block
/// scalars reachable inside block collections (e.g. an empty `strip: >-` entry immediately
/// followed by a sibling `clip: >` at column 0); see AGENT.md's Phase 3 history for the earlier,
/// unbounded version of this scan.
///
/// https://yaml.org/spec/1.2.2/#rule-c-indentation-indicator
#[doc(alias = "c-indentation-indicator")]
pub(super) fn detect_indentation<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, DetectedIndentation, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::header::detect_indentation",
        move |input: &mut Input| {
            let start = input.checkpoint();
            let mut max_empty_spaces = 0usize;
            let mut detected: Option<usize> = None;
            loop {
                let spaces: &str = take_while(0.., b' ').parse_next(input)?;
                let spaces = spaces.len();
                if input.eof_offset() == 0 {
                    max_empty_spaces = max_empty_spaces.max(spaces);
                    break;
                }
                if chars::line_break::<Input, Error>.parse_next(input).is_ok() {
                    max_empty_spaces = max_empty_spaces.max(spaces);
                    continue;
                }
                if spaces <= indent_level.get() {
                    break;
                }
                detected = Some(spaces);
                break;
            }
            if let Some(d) = detected {
                if max_empty_spaces > d {
                    let err = Error::from_input(input).add_context(
                    input,
                    &start,
                    StrContext::Expected(StrContextValue::Description(
                        "a leading all-space line must not have more spaces than the first non-empty line",
                    )),
                );
                    input.reset(&start);
                    return Err(err);
                }
            }
            input.reset(&start);
            Ok(DetectedIndentation {
                content: detected.map(IndentLevel::new),
                bound: IndentLevel::new(detected.unwrap_or(max_empty_spaces)),
            })
        },
    )
}

/// Interpretation of the final line break of a block scalar, per its chomping mode.
///
/// https://yaml.org/spec/1.2.2/#rule-b-chomped-last
#[doc(alias = "b-chomped-last")]
pub(super) fn chomped_last<'i, Input, Error>(
    chomping: ChompingMode,
) -> impl Parser<Input, &'static str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::header::chomped_last",
        move |input: &mut Input| match chomping {
            ChompingMode::Strip => {
                alt((chars::line_break.value(""), eof.value(""))).parse_next(input)
            }
            ChompingMode::Clip | ChompingMode::Keep => {
                alt((chars::break_as_line_feed, eof.value(""))).parse_next(input)
            }
        },
    )
}

/// Interpretation of the trailing empty lines following a block scalar, per its chomping mode.
///
/// https://yaml.org/spec/1.2.2/#rule-l-chomped-empty
#[doc(alias = "l-chomped-empty")]
pub(super) fn chomped_empty<'i, Input, Error>(
    indent_level: IndentLevel,
    chomping: ChompingMode,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::header::chomped_empty",
        move |input: &mut Input| match chomping {
            ChompingMode::Strip | ChompingMode::Clip => {
                strip_empty(indent_level).parse_next(input)?;
                Ok(Cow::Borrowed(""))
            }
            ChompingMode::Keep => keep_empty(indent_level).parse_next(input),
        },
    )
}

/// Trailing empty lines discarded by Strip/Clip chomping.
///
/// https://yaml.org/spec/1.2.2/#rule-l-strip-empty
#[doc(alias = "l-strip-empty")]
fn strip_empty<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::header::strip_empty",
        (
            repeat(
                0..,
                (
                    spaces::indent_less_or_equal(indent_level),
                    chars::line_break.void(),
                ),
            )
            .map(|()| ()),
            opt(trail_comments(indent_level)),
        )
            .void(),
    )
}

/// Trailing empty lines kept by Keep chomping.
///
/// https://yaml.org/spec/1.2.2/#rule-l-keep-empty
#[doc(alias = "l-keep-empty")]
fn keep_empty<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::header::keep_empty", move |input: &mut Input| {
        let count: usize =
            repeat(0.., spaces::line_empty(BlockIn, indent_level)).parse_next(input)?;
        opt(trail_comments(indent_level)).parse_next(input)?;
        if count > 0 {
            Ok(Cow::Owned("\n".repeat(count)))
        } else {
            Ok(Cow::Borrowed(""))
        }
    })
}

/// Trailing comment lines, less indented than the block scalar's content.
///
/// https://yaml.org/spec/1.2.2/#rule-l-trail-comments
#[doc(alias = "l-trail-comments")]
fn trail_comments<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::header::trail_comments",
        (
            spaces::indent_less_than(indent_level),
            spaces::non_break_comment_text,
            spaces::break_comment,
            // `line_comment` can succeed while consuming nothing at EOF (its start-of-line /
            // `eof`-as-break escape hatches) -- same trap as `document.rs`'s `document_prefix`
            // (see AGENT.md Phase 0); guard the same way so `repeat` doesn't trip its
            // must-always-consume invariant.
            repeat(
                0..,
                spaces::line_comment
                    .take()
                    .verify(|s: &str| !s.is_empty())
                    .void(),
            )
            .map(|()| ()),
        )
            .void(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::testing;

    #[test]
    fn block_header_empty() {
        assert_eq!(
            ("literal\n", (None, ChompingMode::Clip)),
            testing::parse(block_header, " # Empty header\nliteral\n").unwrap()
        );
    }

    #[test]
    fn block_header_indentation_then_chomping() {
        assert_eq!(
            ("folded\n", (Some(1), ChompingMode::Clip)),
            testing::parse(block_header, "1 # Indentation indicator\nfolded\n").unwrap()
        );
    }

    #[test]
    fn block_header_chomping_only() {
        assert_eq!(
            ("keep\n", (None, ChompingMode::Keep)),
            testing::parse(block_header, "+ # Chomping indicator\nkeep\n").unwrap()
        );
    }

    #[test]
    fn block_header_both_indicators() {
        assert_eq!(
            ("strip\n", (Some(1), ChompingMode::Strip)),
            testing::parse(block_header, "1- # Both indicators\nstrip\n").unwrap()
        );
        assert_eq!(
            ("strip\n", (Some(1), ChompingMode::Strip)),
            testing::parse(
                block_header,
                "-1 # Both indicators, reversed order\nstrip\n"
            )
            .unwrap()
        );
    }

    #[test]
    fn detect_indentation_from_first_non_empty_line() {
        let (rest, detected) =
            testing::parse(detect_indentation(IndentLevel::initial()), " detected\n").unwrap();
        assert_eq!(" detected\n", rest);
        assert_eq!(Some(IndentLevel::new(1)), detected.content);
        assert_eq!(IndentLevel::new(1), detected.bound);
    }

    #[test]
    fn detect_indentation_rejects_over_indented_leading_empty_line() {
        // Spec example 8.3, case 1: the leading blank line has 2 spaces, more than the 1 space of
        // the first non-empty line that follows.
        testing::parse(detect_indentation(IndentLevel::initial()), "  \n text\n").unwrap_err();
    }

    #[test]
    fn detect_indentation_stops_at_sibling_content_not_indented_further() {
        // A block scalar with no content of its own (e.g. `strip: >-` immediately followed by a
        // sibling `clip: >` at column 0, as in yaml-test-suite case K858) must not mistake that
        // sibling line for its own content: `n=0` (BlockOut root) and the candidate line has 0
        // leading spaces, i.e. not *more* indented than `n`.
        let (rest, detected) =
            testing::parse(detect_indentation(IndentLevel::initial()), "sibling: x\n").unwrap();
        assert_eq!("sibling: x\n", rest);
        assert_eq!(None, detected.content);
        assert_eq!(IndentLevel::new(0), detected.bound);
    }
}
