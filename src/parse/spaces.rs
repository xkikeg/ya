//! Provides spaces parsers and their related types.
//!
//! https://yaml.org/spec/1.2.2/#chapter-6-structural-productions

use std::borrow::Cow;

use winnow::{
    combinator::{alt, delimited, eof, opt, preceded, repeat, terminated, trace},
    error::{StrContext, StrContextValue},
    token::take_while,
    Parser,
};

use super::{
    chars,
    context::{self, YamlContext},
    error::ParserError,
    input::InputStream,
};

type IndentLevelType = usize;

/// Level of indent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndentLevel(IndentLevelType);

impl IndentLevel {
    /// Returns initial indent level used in the documents.
    /// In the reference, represented as `-1`.
    #[inline]
    pub const fn initial() -> Self {
        Self(0)
    }

    /// Creates new IndentLevel out of the given value.
    #[inline]
    pub const fn new(v: IndentLevelType) -> Self {
        Self(v + 1)
    }

    /// Returns the actual width of indentation.
    #[inline]
    pub const fn get(&self) -> IndentLevelType {
        self.0.saturating_sub(1)
    }

    /// Returns 1 smaller level.
    #[inline]
    pub const fn prev(&self) -> Self {
        Self(self.0.saturating_sub(1))
    }
}

impl std::ops::Add<IndentLevelType> for IndentLevel {
    type Output = Self;

    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}

/// Parses the exact given `indent_level`.
/// Noted as `s-indent(n)` in the specification.
///
/// Use [`indent_at_least`] if `s-indent(n+m)` found.
///
/// https://yaml.org/spec/1.2.2/#rule-s-indent
#[doc(alias = "s-indent")]
pub fn indent<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::indent",
        take_while(indent_level.get(), b" ").void(),
    )
}

/// Parses the indent with level `indent_level` at least.
/// Returns the actual level.
/// Used as s-indent(n+m) where n is given
/// https://yaml.org/spec/1.2.2/#rule-s-indent
#[doc(alias = "s-indent")]
pub fn indent_at_least<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, IndentLevel, Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::indent_at_least",
        take_while(indent_level.get().., b" ").map(|s: &str| IndentLevel::new(s.len())),
    )
}

/// Parses the indent with level less than `indent_level`.
/// Returns the actual level.
/// Used as s-indent(n+m) where n is given
/// https://yaml.org/spec/1.2.2/#rule-s-indent
#[doc(alias = "s-indent")]
pub fn indent_less_than<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("spaces::indent_less_than", move |input: &mut Input| {
        if indent_level.get() == 0 {
            // `s-space{0,n-1}` for `n<=0` has an empty valid range (no negative space count
            // exists): this is a legitimate, reachable case (e.g. a root-level block scalar
            // whose content indentation is 0), not a violated invariant, so it must fail
            // recoverably rather than assert/panic -- both call sites (`line_empty`'s
            // `line_prefix` fallback, and `l-trail-comments` via `opt(...)`) already handle a
            // backtrackable failure here correctly.
            return Err(Error::from_input(input));
        }
        take_while(..indent_level.get(), b" ")
            .void()
            .parse_next(input)
    })
}

/// Parses the indent with level less than or equal to `indent_level`.
/// Used as `s-indent-less-or-equal(n)`, e.g. by [block chomping](https://yaml.org/spec/1.2.2/#rule-l-strip-empty).
///
/// https://yaml.org/spec/1.2.2/#rule-s-indent-less-or-equal
#[doc(alias = "s-indent-less-or-equal")]
pub fn indent_less_or_equal<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::indent_less_or_equal",
        take_while(..=indent_level.get(), b" ").void(),
    )
}

/// Parses separate of elements.
///
/// https://yaml.org/spec/1.2.2/#rule-s-separate
#[doc(alias = "s-separate")]
pub fn separate<'i, Context, Input, Error>(
    _context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Context, Input, Error>
where
    Context: YamlContext,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("spaces::separate", Context::separate(indent_level))
}

/// Parses separate lines.
///
/// https://yaml.org/spec/1.2.2/#rule-s-separate-lines
#[doc(alias = "s-separate-lines")]
pub fn separate_lines<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::separate_lines",
        // TODO: fixme use dispatch!
        alt((
            (line_comments, flow_line_prefix(indent_level)).void(),
            separate_in_line,
        )),
    )
}

/// Parses separate in-line.
///
/// https://yaml.org/spec/1.2.2/#rule-s-separate-in-line
#[doc(alias = "s-separate-in-line")]
pub fn separate_in_line<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("spaces::separate_in_line", move |input: &mut Input| {
        let ret = take_while(1.., b" \t").void().parse_next(input);
        if ret.is_err() && input.is_start_of_line() {
            return Ok(());
        }
        ret
    })
    .parse_next(input)
}

/// Parses line prefix.
#[doc(alias = "s-line-prefix")]
pub fn line_prefix<'i, Context, Input, Error>(
    _context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Context, Input, Error>
where
    Context: context::NonKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("spaces::line_prefix", Context::line_prefix(indent_level))
}

impl context::NonKey for context::BlockIn {
    fn line_prefix<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        block_line_prefix(indent_level)
    }
}

impl context::NonKey for context::BlockOut {
    fn line_prefix<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        block_line_prefix(indent_level)
    }
}
impl context::NonKey for context::FlowIn {
    fn line_prefix<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        flow_line_prefix(indent_level)
    }
}

impl context::NonKey for context::FlowOut {
    fn line_prefix<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        flow_line_prefix(indent_level)
    }
}

/// Parses flow line prefix.
///
/// https://yaml.org/spec/1.2.2/#rule-s-block-line-prefix
#[doc(alias = "s-block-line-prefix")]
pub fn block_line_prefix<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("spaces::block_line_prefix", indent(indent_level))
}

/// Parses flow line prefix.
///
/// https://yaml.org/spec/1.2.2/#rule-s-flow-line-prefix
#[doc(alias = "s-flow-line-prefix")]
pub fn flow_line_prefix<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::flow_line_prefix",
        (indent(indent_level), opt(separate_in_line)).void(),
    )
}

/// Parses empty line.
///
/// https://yaml.org/spec/1.2.2/#rule-l-empty
#[doc(alias = "l-empty")]
pub fn line_empty<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, &'static str, Error> + use<'i, Context, Input, Error>
where
    Context: context::NonKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::line_empty",
        preceded(
            alt((
                line_prefix(context, indent_level),
                indent_less_than(indent_level),
            )),
            chars::break_as_line_feed,
        ),
    )
}

/// Line folding.
///
/// Either a simple line break folded as a space,
/// or multiple line breaks turned into breaks.
///
/// https://yaml.org/spec/1.2.2/#rule-b-l-folded
#[doc(alias = "b-l-folded")]
pub fn break_line_folded<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Context: context::NonKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::break_line_folded",
        alt((
            break_line_trimmed(context, indent_level).map(Cow::Owned),
            // this must be a closure, otherwise lifetime ellide doesn't work well.
            #[allow(clippy::redundant_closure)]
            chars::break_as_space.map(|s| Cow::Borrowed(s)),
        )),
    )
}

/// Trimmed break line.
///
/// https://yaml.org/spec/1.2.2/#rule-b-l-trimmed
#[doc(alias = "b-l-trimmed")]
pub fn break_line_trimmed<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, String, Error> + use<'i, Context, Input, Error>
where
    Context: context::NonKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::break_line_trimmed",
        preceded(
            chars::line_break,
            repeat(1.., line_empty(context, indent_level)),
        ),
    )
}

/// Flow folding.
///
/// https://yaml.org/spec/1.2.2/#rule-s-flow-folded
#[doc(alias = "s-flow-folded")]
pub fn flow_folded<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::flow_folded",
        delimited(
            opt(separate_in_line),
            break_line_folded(context::FlowIn, indent_level),
            flow_line_prefix(indent_level),
        ),
    )
}

/// Parses one chunk of comment without line break.
///
/// https://yaml.org/spec/1.2.2/#rule-c-nb-comment-text
#[doc(alias = "c-nb-comment-text")]
pub fn non_break_comment_text<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::non_break_comment_text",
        preceded('#', take_while(0.., chars::is_non_break)),
    )
    .parse_next(input)
}

/// Parses space to break comment.
///
/// https://yaml.org/spec/1.2.2/#rule-s-b-comment
#[doc(alias = "s-b-comment")]
pub fn space_break_comment<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::space_break_comment",
        preceded(
            opt(terminated(separate_in_line, opt(non_break_comment_text))),
            break_comment,
        ),
    )
    .void()
    .parse_next(input)
}

/// Parses start of line.
///
/// https://yaml.org/spec/1.2.2/#a-special-production
#[doc(alias = "start-of-line")]
pub fn start_of_line<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("spaces::start_of_line", move |input: &mut Input| {
        if input.is_start_of_line() {
            Ok(())
        } else {
            Err(Error::from_input(input).add_context(
                input,
                &input.checkpoint(),
                StrContext::Expected(StrContextValue::Description("start of the line")),
            ))
        }
    })
    .parse_next(input)
}

/// Parses break comment.
///
/// https://yaml.org/spec/1.2.2/#rule-b-comment
#[doc(alias = "b-comment")]
pub fn break_comment<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::break_comment",
        alt((chars::line_break, eof)).void(),
    )
    .parse_next(input)
}

/// Parses line-comment.
///
/// https://yaml.org/spec/1.2.2/#rule-l-comment
#[doc(alias = "l-comment")]
pub fn line_comment<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "spaces::line_comment",
        delimited(separate_in_line, opt(non_break_comment_text), break_comment).void(),
    )
    .parse_next(input)
}

/// Parses line-comments.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l-comments
#[doc(alias = "s-l-comments")]
pub fn line_comments<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("spaces::line_comments", move |input: &mut Input| {
        alt((space_break_comment, start_of_line)).parse_next(input)?;
        loop {
            if input.eof_offset() == 0 {
                return Ok(());
            }
            // `l-comment*` is zero-or-more: once real (non-comment, non-blank) sibling content
            // follows -- the common case once a node is followed by another block-collection
            // entry -- `line_comment` fails and the loop must stop gracefully rather than
            // propagate that failure, so roll back to before this attempt.
            let checkpoint = input.checkpoint();
            match line_comment::<Input, Error>.take().parse_next(input) {
                Ok(got) => {
                    if got.is_empty() {
                        return Err(Error::assert(
                            input,
                            "line_comment must consume >= 1 chars when not EOF",
                        ));
                    }
                }
                Err(err) if err.is_backtrack() => {
                    input.reset(&checkpoint);
                    return Ok(());
                }
                Err(err) => return Err(err),
            }
        }
    })
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::testing;

    #[test]
    fn indent_takes_exact() {
        assert_eq!(
            ("_", ()),
            testing::parse(indent(IndentLevel::new(3)), "   _").unwrap()
        );
    }

    #[test]
    fn indent_at_least_consumes_larger() {
        assert_eq!(
            ("_", IndentLevel::new(6)),
            testing::parse(indent_at_least(IndentLevel::new(3)), "      _").unwrap()
        );
    }

    #[test]
    fn flow_folded_parses_single_break_as_space() {
        let got = testing::parse(flow_folded(IndentLevel::initial()), "  \nfoo").unwrap();
        assert_eq!(("foo", Cow::Borrowed(" ")), got);
    }

    #[test]
    fn flow_folded_parses_multi_breaks_as_newline() {
        let got = testing::parse(flow_folded(IndentLevel::initial()), "  \n\nfoo").unwrap();
        assert_eq!(("foo", Cow::Owned("\n".to_string())), got);
    }
}
