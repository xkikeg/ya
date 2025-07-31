use std::borrow::Cow;

use winnow::{
    combinator::{fail, trace},
    Parser,
};

use crate::value::Scalar;

use super::{
    chars, context::FlowOrKey, error::ParserError, input::InputStream, spaces::IndentLevel,
};

/// Plain scalar.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain
#[doc(alias = "ns-plain")]
pub fn plain<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Scalar<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    // TODO: fixme support other scalar type
    Context::non_space_plain(indent_level).map(Scalar::Str)
}

/// Plain text content, multi lines.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-plain-multi-line
#[doc(alias = "ns-plain-multi-line")]
pub(super) fn non_space_plain_multi_line<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
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
pub(super) fn non_space_plain_one_line<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    // TODO: fixme! implement this.
    trace("plain::non_space_plain_one_line", fail).parse_next(input)
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
