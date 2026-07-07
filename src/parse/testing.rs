use winnow::{
    error::{ContextError, ParseError},
    token::rest,
    Parser,
};

use super::input::Input;

// Test-only helper: keep the error unboxed so `.unwrap_err()` assertions can inspect it directly.
#[allow(clippy::result_large_err)]
pub fn parse<'i, P, Output>(
    p: P,
    input: &'i str,
) -> Result<(&'i str, Output), ParseError<Input<'i>, ContextError>>
where
    P: Parser<Input<'i>, Output, ContextError>,
{
    parse_with_input(p, Input::new(input))
}

#[allow(clippy::result_large_err)]
pub fn parse_with_input<'i, P, Output>(
    p: P,
    input: Input<'i>,
) -> Result<(&'i str, Output), ParseError<Input<'i>, ContextError>>
where
    P: Parser<Input<'i>, Output, ContextError>,
{
    let (got, remaining) = (p, rest).parse(input)?;
    Ok((remaining, got))
}
