//! Defines [`ParserError`], which is just trait alias.

use std::num::ParseIntError;

use winnow::{
    error::{AddContext, FromExternalError, StrContext},
    stream::Stream,
};

pub trait ParserError<Input: Stream>:
    winnow::error::ParserError<Input>
    + AddContext<Input, StrContext>
    + FromExternalError<Input, ParseIntError>
{
}

impl<Input: Stream, T> ParserError<Input> for T where
    T: winnow::error::ParserError<Input>
        + AddContext<Input, StrContext>
        + FromExternalError<Input, ParseIntError>
{
}
