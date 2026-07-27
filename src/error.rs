//! Error types shared by [`crate::parse`], [`crate::resolve`] and (with the `serde` feature)
//! [`crate::de`].
//!
//! Syntax errors come in two forms, and which one you get depends on which entry point you call:
//!
//! * [`OwnedParseError`] owns everything it needs to render a diagnostic (message, offset,
//!   line/column, the offending source line), so it is `'static` and can be propagated with `?`
//!   into `Box<dyn std::error::Error>`, `anyhow::Error`, or any error enum, long after the input
//!   `&str` is gone. This is what [`Error`] -- and therefore [`crate::parse`] and
//!   [`crate::de::from_str`] -- carries.
//! * [`ParseError`] keeps the input borrowed, and with it the full winnow
//!   [`ParseError`](winnow::error::ParseError) (and thus the [`ContextError`] and the parse-time
//!   [`Input`] state) for callers who want to inspect more than the rendered message. Reachable by
//!   driving [`crate::parse::yaml_stream`] yourself; see [`ParseError`]'s own docs for the snippet.
//!
//! Both render identically ([`ParseError::into_owned`] loses no part of the message), so the owned
//! form is the right default and the borrowed one is opt-in.

use std::fmt;

use winnow::error::ContextError;

use crate::parse::input::Input;
use crate::resolve::ResolveError;

/// A specialized [`std::result::Result`] for this crate, per
/// [serde's data format conventions](https://serde.rs/conventions.html).
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for [`crate::parse`] and, with the `serde` feature, [`crate::de::from_str`].
///
/// `'static`, [`Clone`] and [`PartialEq`]: it never borrows the parsed input (see the module docs).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// The input isn't valid YAML 1.2.2 syntax.
    Parse(OwnedParseError),
    /// The input parsed, but violates Core Schema tag resolution (e.g. an explicit `!!int` tag on
    /// text that isn't a valid integer).
    Resolve(ResolveError),
    /// The bytes handed to [`crate::de::from_bytes`] aren't valid UTF-8.
    #[cfg(feature = "serde")]
    Utf8(std::str::Utf8Error),
    /// A [`serde::Deserialize`] impl rejected the shape or content of an otherwise-valid node
    /// (e.g. a required struct field is missing, or a custom `Deserialize` impl's own validation
    /// failed). Constructed via [`serde::de::Error::custom`] -- see [`crate::de`].
    #[cfg(feature = "serde")]
    Custom(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(err) => write!(f, "{err}"),
            Error::Resolve(err) => write!(f, "{err}"),
            #[cfg(feature = "serde")]
            Error::Utf8(err) => write!(f, "{err}"),
            #[cfg(feature = "serde")]
            Error::Custom(msg) => write!(f, "{msg}"),
        }
    }
}

// No `source()`: every variant's `Display` already includes the whole underlying message, so
// reporting a source as well would print it twice in a chained renderer like `anyhow`'s
// "Caused by:" section. Same choice `serde_json::Error` makes.
impl std::error::Error for Error {}

impl From<OwnedParseError> for Error {
    fn from(err: OwnedParseError) -> Self {
        Error::Parse(err)
    }
}

impl From<ParseError<'_>> for Error {
    fn from(err: ParseError<'_>) -> Self {
        Error::Parse(err.into_owned())
    }
}

impl From<ResolveError> for Error {
    fn from(err: ResolveError) -> Self {
        Error::Resolve(err)
    }
}

/// A YAML syntax error that still borrows the input it was produced from, keeping the underlying
/// winnow [`ParseError`](winnow::error::ParseError) available for inspection.
///
/// [`crate::parse`] returns the owned form ([`Error::Parse`]) since that's what callers propagate;
/// reach for this one only when you want the winnow error itself, and convert with
/// [`into_owned`](Self::into_owned) once you're done with it:
///
/// ```
/// use winnow::Parser as _;
///
/// let input = "[a, b";
/// let err: ya::ParseError<'_> = ya::parse::yaml_stream::<_, winnow::error::ContextError>
///     .parse(ya::parse::input::Input::new(input))
///     .unwrap_err()
///     .into();
/// assert_eq!(err.line(), 1);
/// assert!(!err.inner().input().original().is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError<'i> {
    inner: winnow::error::ParseError<Input<'i>, ContextError>,
}

impl<'i> ParseError<'i> {
    /// The underlying winnow error, carrying the [`ContextError`] and the parse-time [`Input`].
    pub fn inner(&self) -> &winnow::error::ParseError<Input<'i>, ContextError> {
        &self.inner
    }

    /// The complete input that was parsed.
    pub fn input(&self) -> &'i str {
        self.inner.input().original()
    }

    /// The byte offset into [`input`](Self::input) where parsing failed.
    pub fn offset(&self) -> usize {
        self.inner.offset()
    }

    /// The 1-based line number of [`offset`](Self::offset).
    pub fn line(&self) -> usize {
        locate(self.input(), self.offset()).0
    }

    /// The 1-based column number (in `char`s) of [`offset`](Self::offset).
    pub fn column(&self) -> usize {
        locate(self.input(), self.offset()).1
    }

    /// The text of the line [`offset`](Self::offset) falls on, without its line break.
    pub fn line_text(&self) -> &'i str {
        locate(self.input(), self.offset()).2
    }

    /// Copies out everything needed to render this error, dropping the borrow on the input.
    pub fn into_owned(self) -> OwnedParseError {
        let (line, column, line_text) = locate(self.input(), self.offset());
        OwnedParseError {
            message: self.inner.inner().to_string(),
            offset: self.offset(),
            line,
            column,
            line_text: line_text.to_owned(),
        }
    }
}

impl<'i> From<winnow::error::ParseError<Input<'i>, ContextError>> for ParseError<'i> {
    fn from(inner: winnow::error::ParseError<Input<'i>, ContextError>) -> Self {
        Self { inner }
    }
}

impl fmt::Display for ParseError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (line, column, line_text) = locate(self.input(), self.offset());
        render(f, line, column, line_text, &self.inner.inner().to_string())
    }
}

impl std::error::Error for ParseError<'_> {}

/// A YAML syntax error that owns its diagnostic, so it doesn't borrow the parsed input.
///
/// Produced by [`ParseError::into_owned`] and carried by [`Error::Parse`]. Its [`Display`](fmt::Display)
/// output is identical to the borrowed [`ParseError`]'s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedParseError {
    message: String,
    offset: usize,
    line: usize,
    column: usize,
    line_text: String,
}

impl OwnedParseError {
    /// The parser's own message (winnow's [`ContextError`] rendering), without the source excerpt.
    /// May be empty when the failing parser attached no context.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The byte offset into the original input where parsing failed.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The 1-based line number of [`offset`](Self::offset).
    pub fn line(&self) -> usize {
        self.line
    }

    /// The 1-based column number (in `char`s) of [`offset`](Self::offset).
    pub fn column(&self) -> usize {
        self.column
    }

    /// The text of the line [`offset`](Self::offset) falls on, without its line break.
    pub fn line_text(&self) -> &str {
        &self.line_text
    }
}

impl fmt::Display for OwnedParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(f, self.line, self.column, &self.line_text, &self.message)
    }
}

impl std::error::Error for OwnedParseError {}

/// Renders a syntax error the way winnow's own [`ParseError`](winnow::error::ParseError) does --
/// a position line, the offending source line, and a caret -- so that [`ParseError`] and
/// [`OwnedParseError`] are indistinguishable in output.
fn render(
    f: &mut fmt::Formatter<'_>,
    line: usize,
    column: usize,
    line_text: &str,
    message: &str,
) -> fmt::Result {
    let gutter = line.to_string().len();
    writeln!(f, "parse error at line {line}, column {column}")?;
    writeln!(f, "{:gutter$} |", "")?;
    writeln!(f, "{line} | {line_text}")?;
    write!(f, "{:gutter$} | ", "")?;
    write!(f, "{:width$}^", "", width = column - 1)?;
    if !message.is_empty() {
        write!(f, "\n{message}")?;
    }
    Ok(())
}

/// Locates `offset` within `input`, returning its 1-based line, its 1-based column (counted in
/// `char`s) and the text of that line without its line break.
fn locate(input: &str, offset: usize) -> (usize, usize, &str) {
    // `offset` may point one past the end (winnow reports EOF failures there), and shouldn't be
    // able to land mid-`char` given the parser's token type is `char` -- but clamp anyway rather
    // than risk a panic while building an error message.
    let mut offset = offset.min(input.len());
    while !input.is_char_boundary(offset) {
        offset -= 1;
    }

    let before = &input[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.matches('\n').count() + 1;
    let column = input[line_start..offset].chars().count() + 1;
    let rest = &input[line_start..];
    let line_text = rest
        .split('\n')
        .next()
        .unwrap_or(rest)
        .trim_end_matches('\r');
    (line, column, line_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_reports_line_column_and_line_text() {
        let input = "one\ntwo\nthree\n";
        assert_eq!(locate(input, 0), (1, 1, "one"));
        assert_eq!(locate(input, 3), (1, 4, "one"));
        // Right after the first break: start of line 2.
        assert_eq!(locate(input, 4), (2, 1, "two"));
        assert_eq!(locate(input, 9), (3, 2, "three"));
        // EOF: one past the last break, i.e. an empty final line.
        assert_eq!(locate(input, input.len()), (4, 1, ""));
    }

    #[test]
    fn locate_counts_columns_in_chars_not_bytes() {
        // 'ペ' is 3 bytes; the column after it is 2, not 4.
        let input = "ペン";
        assert_eq!(locate(input, 3), (1, 2, "ペン"));
    }

    #[test]
    fn locate_clamps_an_out_of_range_offset() {
        assert_eq!(locate("ab", 99), (1, 3, "ab"));
    }

    #[test]
    fn owned_parse_error_renders_the_offending_line_with_a_caret() {
        let err = OwnedParseError {
            message: "expected `]`".to_string(),
            offset: 9,
            line: 2,
            column: 6,
            line_text: "foo: [bar".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "parse error at line 2, column 6\n  |\n2 | foo: [bar\n  |      ^\nexpected `]`"
        );
    }
}
