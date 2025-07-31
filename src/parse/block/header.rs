use winnow::{
    combinator::{opt, trace},
    token::one_of,
    Parser,
};

use crate::parse::{error::ParserError, input::InputStream};

/// Chomping mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChompingMode {
    Strip,
    Clip,
    Keep,
}

/// Block chomping indicator.
/// https://yaml.org/spec/1.2.2/#rule-c-chomping-indicator
#[doc(alias = "l+block-mapping")]
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
