use std::borrow::Cow;

use winnow::{
    combinator::{alt, fail, peek, terminated, trace},
    token::one_of,
    Parser,
};

use crate::value::Scalar;

use super::{
    chars,
    context::{FlowOrKey, InOutFlow, Key},
    error::ParserError,
    input::InputStream,
    spaces::IndentLevel,
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
    Context::non_space_plain(indent_level).map(Scalar::SingleStr)
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
    // TODO: fixme! implement this.
    trace("plain::non_space_plain_multi_line", fail)
}

/// Plain text content, one line.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-one-line
#[doc(alias = "ns-plain-one-line")]
pub(super) fn non_space_plain_one_line<'i, Context, Input, Error>(
    context: Context,
) -> impl Parser<Input, &'i str, Error>
where
    Context: Key,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "plain::non_space_plain_one_line",
        (non_space_plain_first(context)).take(),
    )
}

/// Plain first character.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-first
#[doc(alias = "ns-plain-first")]
pub(super) fn non_space_plain_first<'i, Context, Input, Error>(
    context: Context,
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

/// Non-space plain chars.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-char
#[doc(alias = "ns-plain-char")]
pub(super) fn non_space_plain_chars<'i, Context, Input, Error>(
    _context: Context,
) -> impl Parser<Input, &'i str, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    // TODO: fixme! implement this.
    trace("plain::non_space_plain_chars", fail)
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
