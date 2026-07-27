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
//! [`parse_document`] is the top-level entry point for representation-level parsing plus Core
//! Schema tag resolution, and [`parse_stream`] is its multi-document counterpart, parsing one
//! document at a time as you iterate. [`from_str`] (behind the optional `serde` feature)
//! deserializes directly into a caller-supplied type, and [`Deserializer`] does the same for a
//! multi-document stream.
//!
//! ```
//! let doc = ya::parse_document("key: value\n").unwrap();
//! let ya::value::Content::Map(map) = &doc.as_node().value else {
//!     panic!("expected a mapping");
//! };
//! assert_eq!(map.entries()[0].value.as_str(), Some("value"));
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub mod de;
mod documents;
mod error;
pub mod parse;
pub mod resolve;
pub mod value;

pub use documents::{parse_document, parse_stream, Documents};
pub use error::{Error, OwnedParseError, ParseError, Result};

#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub use de::{from_bytes, from_str, Deserializer, NodeDeserializer, StreamDeserializer};

#[cfg(test)]
mod tests {
    use super::*;

    /// The error must not borrow the input, so it can be propagated out of the scope that owns
    /// the parsed `String` -- the whole point of `Error` being `'static`.
    #[test]
    fn error_outlives_the_parsed_input() {
        fn load() -> std::result::Result<(), Box<dyn std::error::Error + 'static>> {
            let owned = String::from("[a, b");
            parse_document(&owned)?;
            Ok(())
        }

        assert!(load().is_err());
    }

    #[test]
    fn syntax_error_reports_its_position() {
        let Error::Parse(err) = parse_document("a: [1, 2\nb: 3\n").unwrap_err() else {
            panic!("expected a syntax error");
        };
        // The offset is where the *stream* gave up -- the start of the document that failed,
        // rather than the exact offending character.
        assert_eq!(err.offset(), 0);
        assert_eq!(err.line(), 1);
        assert_eq!(err.column(), 1);
        assert_eq!(err.line_text(), "a: [1, 2");
        assert!(err.to_string().contains("a: [1, 2"));
    }
}
