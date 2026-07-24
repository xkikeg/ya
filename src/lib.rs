//! `ya` ("yet another YAML parser") is a pure-Rust implementation of the
//! [YAML 1.2.2 specification](https://yaml.org/spec/1.2.2/), built on the
//! [`winnow`](https://docs.rs/winnow) parser-combinator crate.
//!
//! It is implemented **as naive as possible, on purpose**: every production rule in the spec
//! grammar has a corresponding, identically-named, identically-composed Rust parser function
//! rather than a fused or hand-optimized rewrite. Consistency with the specification -- not
//! performance, not idiomatic-Rust cleverness -- is the priority. See
//! [`AGENT.md`](https://github.com/xkikeg/ya/blob/main/AGENT.md) for the full design rationale.
//!
//! [`parse`] is the top-level entry point for representation-level parsing plus Core Schema tag
//! resolution; [`from_str`] (behind the optional `serde` feature) deserializes directly into a
//! caller-supplied type.
//!
//! ```
//! let stream = ya::parse("key: value\n").unwrap();
//! let doc = stream.documents()[0].as_node();
//! let ya::value::Content::Map(map) = &doc.value else {
//!     panic!("expected a mapping");
//! };
//! assert_eq!(map.entries()[0].value.as_str(), Some("value"));
//! ```

#[cfg(feature = "serde")]
pub mod de;
pub mod parse;
pub mod resolve;
pub mod value;

#[cfg(feature = "serde")]
pub use de::from_str;

use winnow::{error::ContextError, Parser as _};

/// Parses a YAML 1.2.2 string into a fully tag-resolved [`value::Stream`].
///
/// This composes the two steps every caller needs -- [`parse::yaml_stream`] (representation-level
/// parsing) and [`resolve::resolve`] (Core Schema tag resolution, AGENT.md Phase 6) -- so this is
/// the intended top-level entry point, rather than reaching into `parse::yaml_stream` +
/// `parse::input::Input` + a winnow `Error` type directly (as `tests/integration_tests.rs` still
/// does, since it needs to distinguish parse failures from resolve failures for its own
/// conformance-category bookkeeping).
///
/// ```
/// let stream = ya::parse("key: value\n").unwrap();
/// let doc = stream.documents()[0].as_node();
/// let ya::value::Content::Map(map) = &doc.value else {
///     panic!("expected a mapping");
/// };
/// assert_eq!(map.entries()[0].value.as_str(), Some("value"));
/// ```
pub fn parse(input: &str) -> Result<value::Stream<'_>, Error<'_>> {
    let stream = parse::yaml_stream::<_, ContextError>
        .parse(parse::input::Input::new(input))
        .map_err(|err| Error::Parse(Box::new(err)))?;
    resolve::resolve(stream).map_err(Error::Resolve)
}

/// Error type for [`parse`].
#[derive(Debug)]
pub enum Error<'i> {
    /// The input isn't valid YAML 1.2.2 syntax.
    // Boxed per clippy::result_large_err: `winnow::error::ParseError` embeds a whole `Input`
    // (anchors + tag handles state), making the unboxed variant far larger than `Resolve`'s.
    Parse(Box<winnow::error::ParseError<parse::input::Input<'i>, ContextError>>),
    /// The input parsed, but violates Core Schema tag resolution (e.g. an explicit `!!int` tag on
    /// text that isn't a valid integer).
    Resolve(resolve::ResolveError),
    /// A [`serde::Deserialize`] impl rejected the shape or content of an otherwise-valid node
    /// (e.g. a required struct field is missing, or a custom `Deserialize` impl's own validation
    /// failed). Only constructible via [`serde::de::Error::custom`], and only when the `serde`
    /// feature is enabled -- see [`de`].
    #[cfg(feature = "serde")]
    Custom(String),
}

impl std::fmt::Display for Error<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(err) => write!(f, "{err}"),
            Error::Resolve(err) => write!(f, "{err}"),
            #[cfg(feature = "serde")]
            Error::Custom(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_composes_yaml_stream_and_resolve() {
        let stream = parse("key: value\n").unwrap();
        let doc = stream.documents()[0].as_node();
        let value::Content::Map(map) = &doc.value else {
            panic!("expected a mapping, got {:?}", doc.value);
        };
        assert_eq!(map.entries()[0].value.as_str(), Some("value"));
    }

    #[test]
    fn parse_reports_syntax_errors() {
        assert!(matches!(parse("[a, b").unwrap_err(), Error::Parse(_)));
    }

    #[test]
    fn parse_reports_resolve_errors() {
        assert!(matches!(parse("!!int foo\n").unwrap_err(), Error::Resolve(_)));
    }
}
