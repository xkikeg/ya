//! Characters library.

use winnow::{
    combinator::{alt, opt, trace},
    error::ParserError,
    stream::{Compare, Stream, StreamIsPartial},
    Parser,
};

/// indicators.
/// https://yaml.org/spec/1.2.2/#rule-c-indicator
#[doc(alias = "c-indicator")]
pub const INDICATORS: &[u8] = br#"-?:,[]{}#&*!|>'"%@`"#;

/// Byte-Order-Mark.
pub const BOM: char = '\u{feff}';

/// non-break chars.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-char
#[inline]
pub fn is_non_break(c: char) -> bool {
    c == '\t' || c.is_ascii_graphic() || (!c.is_ascii() && !c.is_control())
}

/// non-break non-space chars.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-char
#[inline]
pub fn is_non_space(c: char) -> bool {
    is_non_break(c) && !is_white_space(c)
}

/// White space, ascii tab or space.
///
/// https://yaml.org/spec/1.2.2/#rule-s-white
#[doc(alias = "s-white")]
#[inline]
pub const fn is_white_space(c: char) -> bool {
    c == '\t' || c == ' '
}

/// Flow indicator chars.
///
/// https://yaml.org/spec/1.2.2/#rule-c-flow-indicator
#[doc(alias = "c-flow-indicator")]
#[inline]
pub const fn is_flow_indicator(c: char) -> bool {
    matches!(c, ',' | '[' | ']' | '{' | '}')
}

/// Indicator chars.
///
/// https://yaml.org/spec/1.2.2/#rule-c-indicator
#[doc(alias = "c-flow-indicator")]
#[inline]
pub const fn is_indicator(c: char) -> bool {
    matches!(
        c,
        '-' | '?'
            | ':'
            | ','
            | '['
            | ']'
            | '{'
            | '}'
            | '#'
            | '&'
            | '*'
            | '!'
            | '|'
            | '>'
            | '\''
            | '"'
            | '%'
            | '@'
            | '`'
    )
}

/// JSON compatible chars.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-json
#[doc(alias = "nb-json")]
#[inline]
pub fn is_nb_json(c: char) -> bool {
    c == '\x7f' || !c.is_ascii_control()
}

/// line_break, similar to winnow's default `line_ending` but accepts single `\r`.
///
/// https://yaml.org/spec/1.2.2/#rule-b-break
#[doc(alias = "b-break")]
pub fn line_break<Input, Error>(input: &mut Input) -> Result<<Input as Stream>::Slice, Error>
where
    Input: StreamIsPartial + Stream + Compare<&'static str>,
    Error: ParserError<Input>,
{
    trace("chars::line_break", alt(("\n", ("\r", opt("\n")).take()))).parse_next(input)
}

/// Line break consumed as line feed `\n`.
///
/// https://yaml.org/spec/1.2.2/#rule-b-as-line-feed
#[doc(alias = "b-as-line-feed")]
pub fn break_as_line_feed<Input, Error>(input: &mut Input) -> Result<&'static str, Error>
where
    Input: StreamIsPartial + Stream + Compare<&'static str>,
    Error: ParserError<Input>,
{
    trace("chars::break_as_line_feed", line_break.value("\n")).parse_next(input)
}

/// Line break consumed as space.
///
/// https://yaml.org/spec/1.2.2/#rule-b-as-space
#[doc(alias = "b-as-space")]
pub fn break_as_space<Input, Error>(input: &mut Input) -> Result<&'static str, Error>
where
    Input: StreamIsPartial + Stream + Compare<&'static str>,
    Error: ParserError<Input>,
{
    trace("chars::break_as_space", line_break.value(" ")).parse_next(input)
}
