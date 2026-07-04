use std::borrow::Cow;

use winnow::{
    combinator::{alt, not, opt, peek, preceded, repeat, terminated, trace},
    token::{one_of, take_while},
    Parser,
};

use crate::value::Scalar;

use super::{
    chars,
    context::{FlowOrKey, InOutFlow},
    document,
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// Plain scalar.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain
#[doc(alias = "ns-plain")]
pub fn plain<'i, Context, Input, Error>(
    _context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Scalar<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    // For the scalar, I'd say schema would be applied at later stage,
    // once user want to have a value.
    trace(
        "plain::plain",
        preceded(
            // The spec formally scopes `c-forbidden` as an exclusion on `l-bare-document`
            // (https://yaml.org/spec/1.2.2/#rule-l-bare-document): a bare document's content
            // must not start with a `---`/`...` marker line. We check it here instead of at
            // that level because the only way a forbidden line could ever end up consumed as
            // scalar content is if a plain scalar's line folding swallows it -- either as the
            // scalar's very first line (right here) or as a later continuation line (the same
            // check in `space_non_space_plain_next_line` below). Guarding both of those spots
            // is behaviorally equivalent to the spec's `l-bare-document`-level exclusion,
            // without threading a "we're inside a bare document" flag down through
            // flow_node/flow_content/flow_yaml_content just to reach this one rule. Block
            // scalars/collections will need their own guard once they exist (Phase 3/4).
            not(document::forbidden),
            Context::non_space_plain(indent_level),
        )
        .map(Scalar::Plain),
    )
}

/// Plain text content, multi lines.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-multi-line
#[doc(alias = "ns-plain-multi-line")]
pub(super) fn non_space_plain_multi_line<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Context: InOutFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::non_space_plain_multi_line",
        move |input: &mut Input| {
            let mut current: Cow<str> = non_space_plain_one_line(context)
                .map(Cow::Borrowed)
                .parse_next(input)?;
            loop {
                let next = opt(space_non_space_plain_next_line(context, indent_level))
                    .parse_next(input)?;
                let (fold, line) = match next {
                    None => return Ok(current),
                    Some(v) => v,
                };
                current.to_mut().push_str(&fold);
                current.to_mut().push_str(line);
            }
        },
    )
}

/// Plain text, next line content.
///
/// https://yaml.org/spec/1.2.2/#rule-s-ns-plain-next-line
#[doc(alias = "s-ns-plain-next-line")]
pub(super) fn space_non_space_plain_next_line<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, (Cow<'i, str>, &'i str), Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::space_non_space_plain_next_line",
        (
            spaces::flow_folded(indent_level),
            // A multi-line plain scalar must not swallow a `---`/`...` document marker line
            // either; see the comment on `plain()` above for why this check lives here rather
            // than at `l-bare-document` where the spec formally scopes it.
            not(document::forbidden).void(),
            (
                non_space_plain_char(context),
                non_break_non_space_plain_in_line(context),
            )
                .take(),
        )
            .map(|(fold, (), line)| (fold, line)),
    )
}

/// Plain text content, one line.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-one-line
#[doc(alias = "ns-plain-one-line")]
pub(super) fn non_space_plain_one_line<'i, Context, Input, Error>(
    context: Context,
) -> impl Parser<Input, &'i str, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::non_space_plain_one_line",
        (
            non_space_plain_first(context),
            non_break_non_space_plain_in_line(context),
        )
            .take(),
    )
}

/// Plain first character.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-first
#[doc(alias = "ns-plain-first")]
pub(super) fn non_space_plain_first<'i, Context, Input, Error>(
    _context: Context,
) -> impl Parser<Input, char, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::non_space_plain_first",
        alt((
            one_of(|c| chars::is_non_space(c) && !chars::is_indicator(c)),
            terminated(one_of(b"?:-"), peek(one_of(Context::is_plain_safe))),
        )),
    )
}

/// In-line plain text (no leading/trailing line break).
///
/// https://yaml.org/spec/1.2.2/#rule-nb-ns-plain-in-line
#[doc(alias = "nb-ns-plain-in-line")]
pub(super) fn non_break_non_space_plain_in_line<'i, Context, Input, Error>(
    context: Context,
) -> impl Parser<Input, &'i str, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::non_break_non_space_plain_in_line",
        repeat(
            0..,
            (
                take_while(0.., chars::is_white_space),
                non_space_plain_char(context),
            ),
        )
        .map(|()| ())
        .take(),
    )
}

/// Non-space plain chars.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-char
#[doc(alias = "ns-plain-char")]
pub(super) fn non_space_plain_char<'i, Context, Input, Error>(
    _context: Context,
) -> impl Parser<Input, char, Error> + use<'i, Context, Input, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::non_space_plain_char",
        alt((
            one_of(|c: char| Context::is_plain_safe(c) && c != ':' && c != '#'),
            comment_indicator_after_non_space,
            terminated(one_of(':'), peek(one_of(Context::is_plain_safe))),
        )),
    )
}

/// Matches a `#` only when the immediately preceding character is an `ns-char`; the
/// `[lookbehind = ns-char] '#'` alternative of `ns-plain-char`, since winnow has no built-in
/// lookbehind.
fn comment_indicator_after_non_space<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<char, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::comment_indicator_after_non_space",
        move |input: &mut Input| {
            if !input.previous_char().is_some_and(chars::is_non_space) {
                return Err(Error::from_input(input));
            }
            one_of('#').parse_next(input)
        },
    )
    .parse_next(input)
}

/// Plain safe chars out of flow context.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-safe-out.
#[doc(alias = "ns-plain-safe-out")]
#[inline]
pub(super) fn is_plain_safe_out(c: char) -> bool {
    chars::is_non_space(c)
}

/// Plain safe chars in of flow context.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-safe-in.
#[doc(alias = "ns-plain-safe-in")]
#[inline]
pub(super) fn is_plain_safe_in(c: char) -> bool {
    is_plain_safe_out(c) && !chars::is_flow_indicator(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::{context, testing};

    #[test]
    fn plain_one_line_flow_key() {
        assert_eq!(
            (" ", "abc"),
            testing::parse(non_space_plain_one_line(context::FlowKey), "abc ").unwrap()
        );
    }

    #[test]
    fn plain_stops_before_colon_space() {
        // `ns-plain-char` only allows `:` when followed by an `ns-plain-safe` char; `key: value`
        // must stop the plain scalar right before the `:` that introduces a mapping value.
        assert_eq!(
            (": value", "key"),
            testing::parse(non_space_plain_one_line(context::FlowOut), "key: value").unwrap()
        );
    }

    #[test]
    fn plain_allows_colon_without_following_space() {
        // `a:b` is one plain scalar, not a mapping, because `:` isn't followed by whitespace.
        assert_eq!(
            ("", "a:b"),
            testing::parse(non_space_plain_one_line(context::FlowOut), "a:b").unwrap()
        );
    }

    #[test]
    fn plain_hash_mid_word_is_not_a_comment() {
        // lookbehind arm: `#` preceded by a non-space char is part of the plain scalar.
        assert_eq!(
            ("", "foo#bar"),
            testing::parse(non_space_plain_one_line(context::FlowOut), "foo#bar").unwrap()
        );
    }

    #[test]
    fn plain_hash_after_space_starts_a_comment() {
        // `#` preceded by whitespace is a comment, so the plain scalar stops before the space.
        assert_eq!(
            (" #bar", "foo"),
            testing::parse(non_space_plain_one_line(context::FlowOut), "foo #bar").unwrap()
        );
    }

    #[test]
    fn plain_negative_number() {
        assert_eq!(
            ("", "-123"),
            testing::parse(non_space_plain_one_line(context::FlowOut), "-123").unwrap()
        );
    }

    #[test]
    fn plain_leading_colon_flow_indicator() {
        assert_eq!(
            ("", "::vector"),
            testing::parse(non_space_plain_one_line(context::FlowOut), "::vector").unwrap()
        );
    }

    /// Spec example 7.12 "Plain Lines": a blank line folds to `\n`, a single line break folds to
    /// a space, and leading/trailing white space around each continuation line is trimmed.
    #[test]
    fn plain_multi_line_folds_and_trims() {
        let input = "1st non-empty\n\n 2nd non-empty \n\t3rd non-empty";
        assert_eq!(
            (
                "",
                Cow::Owned("1st non-empty\n2nd non-empty 3rd non-empty".to_string())
            ),
            testing::parse(
                non_space_plain_multi_line(context::FlowOut, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }

    #[test]
    fn plain_multi_line_single_char_stays_borrowed() {
        assert_eq!(
            ("", Cow::Borrowed("abc")),
            testing::parse(
                non_space_plain_multi_line(context::FlowOut, IndentLevel::initial()),
                "abc"
            )
            .unwrap()
        );
    }

    #[test]
    fn plain_multi_line_does_not_swallow_document_end_marker() {
        let input = "foo\n...\n";
        assert_eq!(
            ("\n...\n", Cow::Borrowed("foo")),
            testing::parse(
                non_space_plain_multi_line(context::FlowOut, IndentLevel::initial()),
                input
            )
            .unwrap()
        );
    }
}
