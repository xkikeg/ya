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

use crate::value::Scalar;

use super::{
    context::{self, FlowOrKey},
    double,
    error::ParserError,
    input::InputStream,
    single,
    spaces::IndentLevel,
};

/// Scalar parses a scalar content.
///
/// TODO: remove this.
pub fn scalar<'i, Input, Error>(input: &mut Input) -> winnow::Result<Scalar<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    // first try implementing simple string.
    // TODO: use dispatch
    trace(
        "scalar",
        double::double_quoted(context::FlowIn, IndentLevel::initial()).map(Scalar::Str),
    )
    .parse_next(input)
}

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
    delimited(
        one_of('\''),
        single::non_break_single_text(context, indent_level),
        one_of('\''),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::testing;

    #[test]
    fn scalar_plain_str() {
        let input = r#"foo"#;
        let got = testing::parse(scalar, input).unwrap();

        assert_eq!(("", Scalar::Str(Cow::Borrowed("foo"))), got);
    }
}
