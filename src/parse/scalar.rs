//! Provides scalar parsers including both flow and block.
//!
//! Flow scalar: https://yaml.org/spec/1.2.2/#73-flow-scalar-styles
//! Block scalar: https://yaml.org/spec/1.2.2/#81-block-scalar-styles

use std::borrow::Cow;

use winnow::{
    combinator::{delimited, trace},
    token::one_of,
    Parser,
};

use super::{
    context::FlowOrKey, error::ParserError, input::InputStream, single, spaces::IndentLevel,
};

/// Single quoted text.
///
/// https://yaml.org/spec/1.2.2/#rule-c-single-quoted
#[doc(alias = "c-single-quoted")]
pub fn single_quoted<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "scalar::single_quoted",
        delimited(
            one_of('\''),
            single::non_break_single_text(context, indent_level),
            one_of('\''),
        ),
    )
}
