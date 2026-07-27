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
//! caller-supplied type, and [`Deserializer`] does the same for a multi-document stream.
//!
//! ```
//! let stream = ya::parse("key: value\n").unwrap();
//! let doc = stream.documents()[0].as_node();
//! let ya::value::Content::Map(map) = &doc.value else {
//!     panic!("expected a mapping");
//! };
//! assert_eq!(map.entries()[0].value.as_str(), Some("value"));
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub mod de;
mod error;
pub mod parse;
pub mod resolve;
pub mod value;

pub use error::{Error, OwnedParseError, ParseError, Result};

#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub use de::{from_bytes, from_str, Deserializer, NodeDeserializer, StreamDeserializer};

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
pub fn parse(input: &str) -> Result<value::Stream<'_>> {
    let stream = parse::yaml_stream::<_, ContextError>
        .parse(parse::input::Input::new(input))
        .map_err(|err| Error::Parse(ParseError::from(err).into_owned()))?;
    Ok(resolve::resolve(stream)?)
}

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
        assert!(matches!(
            parse("!!int foo\n").unwrap_err(),
            Error::Resolve(_)
        ));
    }

    /// The error must not borrow the input, so it can be propagated out of the scope that owns
    /// the parsed `String` -- the whole point of `Error` being `'static`.
    #[test]
    fn error_outlives_the_parsed_input() {
        fn load() -> std::result::Result<(), Box<dyn std::error::Error + 'static>> {
            let owned = String::from("[a, b");
            parse(&owned)?;
            Ok(())
        }

        assert!(load().is_err());
    }

    #[test]
    fn syntax_error_reports_its_position() {
        let Error::Parse(err) = parse("a: [1, 2\nb: 3\n").unwrap_err() else {
            panic!("expected a syntax error");
        };
        // The offset is winnow's: where the *top-level* parser gave up, which for this grammar is
        // the start of the document that failed rather than the exact offending character.
        assert_eq!(err.offset(), 0);
        assert_eq!(err.line(), 1);
        assert_eq!(err.column(), 1);
        assert_eq!(err.line_text(), "a: [1, 2");
        assert!(err.to_string().contains("a: [1, 2"));
    }
}
