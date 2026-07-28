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
//!
//! Every error that knows *where* it came from carries an [`Excerpt`] -- the offending source
//! lines, the position within them, and enough bookkeeping to render them through
//! [`annotate_snippets`]. That covers resolution errors ([`crate::resolve::ResolveError`]) and,
//! with the `serde` feature, failed `Deserialize` impls, not just syntax errors: the parser records
//! a [`Span`] on every node it produces, so anything raised against a node can point at its text.

use std::fmt;
use std::ops::Range;

use annotate_snippets::{AnnotationKind, Group, Level, Renderer};
use winnow::error::ContextError;

use crate::parse::input::Input;
use crate::resolve::ResolveError;
use crate::value::Span;

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
    /// A single document was expected, but the input holds a `---`/`...`-separated stream of more
    /// than one. Use [`crate::parse_stream`] for those (or, with the `serde` feature,
    /// `Deserializer::into_iter`).
    MultipleDocuments,
    /// The bytes handed to [`crate::de::from_bytes`] aren't valid UTF-8.
    #[cfg(feature = "serde")]
    Utf8(std::str::Utf8Error),
    /// A [`serde::Deserialize`] impl rejected the shape or content of an otherwise-valid node
    /// (e.g. a required struct field is missing, or a `Deserialize` impl's own validation
    /// failed). This is what serde's catch-all [`serde::de::Error::custom`] constructs for this
    /// crate -- see [`crate::de`].
    ///
    /// `excerpt` points at the node being deserialized when the failure happened, whenever the
    /// deserializer knows the source text it came from (which everything reached through
    /// `from_str`/`from_bytes`/`Deserializer` does; a bare
    /// [`NodeDeserializer::new`](crate::de::NodeDeserializer::new) does not).
    #[cfg(feature = "serde")]
    Deserialize {
        message: String,
        excerpt: Option<Excerpt>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(err) => write!(f, "{err}"),
            Error::Resolve(err) => write!(f, "{err}"),
            Error::MultipleDocuments => write!(
                f,
                "expected a single YAML document, found a stream of more than one"
            ),
            #[cfg(feature = "serde")]
            Error::Utf8(err) => write!(f, "{err}"),
            #[cfg(feature = "serde")]
            Error::Deserialize { message, excerpt } => render(f, message, excerpt.as_ref()),
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
        OwnedParseError {
            message: self.inner.inner().to_string(),
            excerpt: Excerpt::at(self.input(), self.offset()),
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
        let excerpt = Excerpt::at(self.input(), self.offset());
        render(f, &self.inner.inner().to_string(), Some(&excerpt))
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
    excerpt: Excerpt,
}

impl OwnedParseError {
    /// Builds an error for `offset` within `input` directly, for a parse driven by hand rather than
    /// by winnow's [`Parser::parse`](winnow::Parser::parse).
    ///
    /// The borrowed [`ParseError`] can't serve that case: it wraps a winnow
    /// [`ParseError`](winnow::error::ParseError), whose only constructor is private to winnow, so
    /// the only way to obtain one is to let `Parser::parse` build it. Nothing is lost -- this
    /// records the same message/offset/location that [`ParseError::into_owned`] copies out.
    pub(crate) fn from_parts(input: &str, offset: usize, message: String) -> Self {
        Self {
            message,
            excerpt: Excerpt::at(input, offset),
        }
    }

    /// The parser's own message (winnow's [`ContextError`] rendering), without the source excerpt.
    /// May be empty when the failing parser attached no context.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The source this error points at.
    pub fn excerpt(&self) -> &Excerpt {
        &self.excerpt
    }

    /// The byte offset into the original input where parsing failed.
    pub fn offset(&self) -> usize {
        self.excerpt.span().start()
    }

    /// The 1-based line number of [`offset`](Self::offset).
    pub fn line(&self) -> usize {
        self.excerpt.line()
    }

    /// The 1-based column number (in `char`s) of [`offset`](Self::offset).
    pub fn column(&self) -> usize {
        self.excerpt.column()
    }

    /// The text of the line [`offset`](Self::offset) falls on, without its line break.
    pub fn line_text(&self) -> &str {
        self.excerpt.line_text()
    }
}

impl fmt::Display for OwnedParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(f, &self.message, Some(&self.excerpt))
    }
}

impl std::error::Error for OwnedParseError {}

/// The title used when the failing parser attached no context of its own, since a rendered
/// diagnostic always needs one. winnow's [`ContextError`] is frequently empty -- notably for the
/// "there is input left that isn't a document" failure, where the position is the whole story.
const UNTITLED: &str = "invalid YAML syntax";

/// The source text an error points at: the input lines a [`Span`] covers, plus where within them
/// it falls.
///
/// Only the covered lines are kept, not the whole input, so an error stays cheap to propagate --
/// [`annotate_snippets`] renders identically either way, given a span rebased onto what it's shown.
// Boxed: an `Excerpt` is ~70 bytes, and it ends up inside `Error`, which every `crate::Result`
// carries. One allocation on the error path (which already allocates the excerpt's text) keeps
// every `Ok` path's `Result` small.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excerpt(Box<ExcerptData>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExcerptData {
    span: Span,
    line: usize,
    column: usize,
    text: String,
    span_in_text: Range<usize>,
}

impl Excerpt {
    /// Builds the excerpt of `input` covering `span`.
    pub(crate) fn new(input: &str, span: Span) -> Self {
        let start = clamp(input, span.start());
        let end = clamp(input, span.end().max(span.start()));
        let (line, column, _) = locate(input, start);

        let line_start = input[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = input[end..]
            .find('\n')
            .map_or(input.len(), |i| end + i)
            .max(start);
        let text = input[line_start..line_end].trim_end_matches('\r');

        Self(Box::new(ExcerptData {
            span: Span::new(start, end),
            line,
            column,
            text: text.to_owned(),
            span_in_text: (start - line_start)..(end - line_start).min(text.len()),
        }))
    }

    /// Builds a zero-width excerpt pointing at `offset`, for a failure that has a position but no
    /// extent -- which is every syntax error, since winnow reports where a parser gave up rather
    /// than a range it rejected.
    pub(crate) fn at(input: &str, offset: usize) -> Self {
        Self::new(input, Span::new(offset, offset))
    }

    /// The input range this excerpt covers.
    pub fn span(&self) -> Span {
        self.0.span
    }

    /// The 1-based line number the span starts on.
    pub fn line(&self) -> usize {
        self.0.line
    }

    /// The 1-based column number (in `char`s) the span starts at.
    pub fn column(&self) -> usize {
        self.0.column
    }

    /// The text of the line the span starts on, without its line break.
    pub fn line_text(&self) -> &str {
        self.text().split('\n').next().unwrap_or(self.text())
    }

    /// The complete excerpted source: every line the span touches.
    pub fn text(&self) -> &str {
        &self.0.text
    }

    /// The span, rebased onto [`text`](Self::text) for rendering.
    fn span_in_text(&self) -> Range<usize> {
        self.0.span_in_text.clone()
    }
}

/// Renders `title`, and the source it points at when there is one, through
/// [`annotate_snippets`]'s plain (uncoloured) renderer -- a library caller has no terminal context,
/// so escape sequences would be noise in a log or a `Box<dyn Error>` chain.
pub(crate) fn render(
    f: &mut fmt::Formatter<'_>,
    title: &str,
    excerpt: Option<&Excerpt>,
) -> fmt::Result {
    let title = if title.is_empty() { UNTITLED } else { title };
    let Some(excerpt) = excerpt else {
        return write!(f, "{title}");
    };
    let group = Group::with_title(Level::ERROR.primary_title(title)).element(
        annotate_snippets::Snippet::source(excerpt.text())
            .line_start(excerpt.line())
            .annotation(AnnotationKind::Primary.span(excerpt.span_in_text())),
    );
    write!(f, "{}", Renderer::plain().render(&[group]))
}

/// Clamps `offset` into `input`, and off a `char` boundary onto the one below it. An offset one
/// past the end is normal (winnow reports EOF failures there); anything else would be a bug, but
/// building an error message is the last place to panic over it.
fn clamp(input: &str, offset: usize) -> usize {
    let mut offset = offset.min(input.len());
    while !input.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Locates `offset` within `input`, returning its 1-based line, its 1-based column (counted in
/// `char`s) and the text of that line without its line break.
fn locate(input: &str, offset: usize) -> (usize, usize, &str) {
    let offset = clamp(input, offset);
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
    fn excerpt_covers_the_lines_its_span_touches() {
        let input = "one\ntwo\nthree\n";
        let excerpt = Excerpt::new(input, Span::new(4, 7));
        assert_eq!(excerpt.line(), 2);
        assert_eq!(excerpt.column(), 1);
        assert_eq!(excerpt.text(), "two");
        assert_eq!(excerpt.span_in_text(), 0..3);

        // A span crossing a break excerpts both lines, and the rebased span still covers it.
        let excerpt = Excerpt::new(input, Span::new(5, 9));
        assert_eq!(excerpt.line(), 2);
        assert_eq!(excerpt.column(), 2);
        assert_eq!(excerpt.text(), "two\nthree");
        assert_eq!(&excerpt.text()[excerpt.span_in_text()], "wo\nt");
    }

    #[test]
    fn excerpt_at_points_at_a_position_with_no_extent() {
        let excerpt = Excerpt::at("foo: [bar\n", 5);
        assert_eq!((excerpt.line(), excerpt.column()), (1, 6));
        assert_eq!(excerpt.line_text(), "foo: [bar");
        assert!(excerpt.span().is_empty());
    }

    /// An offset one past the end is normal -- winnow reports EOF failures there.
    #[test]
    fn excerpt_handles_an_offset_at_end_of_input() {
        let excerpt = Excerpt::at("ab", 2);
        assert_eq!((excerpt.line(), excerpt.column()), (1, 3));
        assert_eq!(excerpt.text(), "ab");
        assert_eq!(excerpt.span_in_text(), 2..2);
    }

    /// One of the three exact-output tests (see also `resolve` and `de`): a rebased span, an
    /// off-by-one `line_start` or a caret in the wrong column all render wrong while every
    /// accessor still reports the right numbers, so only comparing the rendering catches them.
    #[test]
    fn owned_parse_error_renders_the_offending_line_with_a_caret() {
        let err =
            OwnedParseError::from_parts("key: value\nfoo: [bar\n", 16, "expected `]`".to_string());
        assert_eq!(
            err.to_string(),
            "\
error: expected `]`
  |
2 | foo: [bar
  |      ^"
        );
    }

    /// A parser that attached no context still needs a title; the position is the diagnostic.
    #[test]
    fn parse_error_without_a_message_still_renders_a_title() {
        let err = OwnedParseError::from_parts("[a, b", 0, String::new());
        assert!(err.to_string().starts_with("error: invalid YAML syntax"));
        assert!(err.to_string().contains("[a, b"));
    }
}
