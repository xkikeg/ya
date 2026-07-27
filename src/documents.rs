//! Lazy, document-at-a-time parsing: [`parse_stream`], [`parse_document`] and the [`Documents`]
//! iterator behind both.
//!
//! [`crate::parse::yaml_stream`] parses a whole stream into a `Vec<Document>` before returning
//! anything, which is what the conformance harness wants and what the spec production
//! `l-yaml-stream` describes. This module drives the *same* parsers one document at a time instead
//! (see [`crate::parse::document`]'s `stream_head`/`stream_step`), so a multi-document stream costs
//! only its largest single document in peak memory, and a syntax error in the first document
//! surfaces without parsing the rest.
//!
//! This is laziness over documents, not over input: the whole source is already a `&str` in memory.
//! Reading lazily from an [`std::io::Read`] would mean owning a buffer, which is incompatible with
//! the `Cow::Borrowed` zero-copy discipline the parser is built on.

use winnow::error::ContextError;
use winnow::stream::Stream as _;

use crate::parse::document::{self, StreamStep};
use crate::parse::input::Input;
use crate::resolve::resolve_document;
use crate::value::{Content, Document, Node, StandardTag, Tag};
use crate::{Error, OwnedParseError};

/// Parses `input` into an iterator over its documents, one parsed at a time.
///
/// Each item is fully tag-resolved (i.e. [`crate::resolve::resolve_document`] has run on it), the
/// same way [`parse_document`] resolves the single document it returns.
///
/// ```
/// let docs: ya::Result<Vec<_>> = ya::parse_stream("a: 1\n---\na: 2\n").collect();
/// assert_eq!(docs.unwrap().len(), 2);
/// ```
pub fn parse_stream(input: &str) -> Documents<'_> {
    Documents {
        input: Input::new(input),
        state: State::Head,
    }
}

/// Parses `input`, which must hold exactly one document, into that document.
///
/// An input with no documents at all (empty, or nothing but comments) yields an implicit null
/// document, per the Core Schema's treatment of an empty document; an input holding more than one
/// yields [`Error::MultipleDocuments`], and [`parse_stream`] is the entry point for those.
///
/// ```
/// let doc = ya::parse_document("key: value\n").unwrap();
/// let ya::value::Content::Map(map) = &doc.as_node().value else {
///     panic!("expected a mapping");
/// };
/// assert_eq!(map.entries()[0].value.as_str(), Some("value"));
///
/// assert!(ya::parse_document("").unwrap().as_node().is_null());
/// ```
pub fn parse_document(input: &str) -> crate::Result<Document<'_>> {
    single_document(parse_stream(input))
}

/// Reduces `documents` to the single document [`parse_document`] documents, shared with
/// `crate::de`'s single-document deserialization so the empty-stream-is-null rule lives in one
/// place.
pub(crate) fn single_document(mut documents: Documents<'_>) -> crate::Result<Document<'_>> {
    let first = match documents.next() {
        None => {
            return Ok(Document::new(Node::new(
                Content::Empty,
                Tag::Standard(StandardTag::Null),
            )))
        }
        Some(doc) => doc?,
    };
    match documents.next() {
        None => Ok(first),
        // A failure while looking for a second document is the more specific complaint of the two,
        // so report it rather than "there is more than one document here".
        Some(Err(err)) => Err(err),
        Some(Ok(_)) => Err(Error::MultipleDocuments),
    }
}

/// An iterator over the documents of a YAML input, parsing each one only when it's asked for.
///
/// Created by [`parse_stream`]. A parse failure is yielded as a single `Err` item and ends the
/// iteration: the stream position after a failure isn't meaningful, so there's nothing to resume
/// from. A [tag-resolution](crate::resolve) failure does *not* end it -- that document parsed fine,
/// so the ones after it are still reachable.
#[must_use = "`Documents` is an iterator, and parses nothing unless consumed"]
pub struct Documents<'i> {
    input: Input<'i>,
    state: State,
}

/// Which part of `l-yaml-stream` [`Documents::next`] resumes at.
enum State {
    /// The stream's `l-document-prefix* l-any-document?` head, parsed once.
    Head,
    /// The trailing `( ... )*` group, one iteration per [`Documents::next`] call at most.
    Body,
    /// The stream ended, or failed. [`Documents::next`] returns `None` from here on.
    Done,
}

impl<'i> Iterator for Documents<'i> {
    type Item = crate::Result<Document<'i>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.state {
                State::Done => return None,
                State::Head => {
                    self.state = State::Body;
                    match document::stream_head::<_, ContextError>(&mut self.input) {
                        Err(err) => return Some(Err(self.fail(&err))),
                        Ok(Some(doc)) => return Some(self.resolve(doc)),
                        // No document at the head of the stream; carry on into the body.
                        Ok(None) => {}
                    }
                }
                State::Body => match document::stream_step::<_, ContextError>(&mut self.input) {
                    Err(err) => return Some(Err(self.fail(&err))),
                    Ok(StreamStep::Document(doc)) => return Some(self.resolve(doc)),
                    // This iteration consumed a BOM/comment/`...` suffix but produced no document.
                    Ok(StreamStep::Skipped) => {}
                    Ok(StreamStep::End) => return self.end_of_stream().map(Err),
                },
            }
        }
    }
}

impl std::iter::FusedIterator for Documents<'_> {}

impl<'i> Documents<'i> {
    /// The complete input being parsed, regardless of how far the iterator has advanced.
    pub fn source(&self) -> &'i str {
        self.input.original()
    }

    /// Resolves `document`'s tags, pointing any failure at the source it came from -- the node's
    /// [`Span`](crate::value::Span) means nothing to a caller without the input to read it against,
    /// and this iterator is holding that input.
    fn resolve(&self, document: Document<'i>) -> crate::Result<Document<'i>> {
        resolve_document(document).map_err(|err| Error::Resolve(err.located(self.source())))
    }

    /// The current byte offset into the original input.
    ///
    /// `eof_offset` counts the *remaining* input, and this crate's stream is `&str`-backed, so its
    /// offsets are byte offsets into that same string.
    fn offset(&self) -> usize {
        self.input.original().len() - self.input.eof_offset()
    }

    /// Ends the iteration with a parse failure at the current position.
    fn fail(&mut self, err: &ContextError) -> Error {
        self.state = State::Done;
        Error::Parse(OwnedParseError::from_parts(
            self.input.original(),
            self.offset(),
            err.to_string(),
        ))
    }

    /// Handles the parser reporting no further stream elements: either the input is exhausted (the
    /// stream is simply over, and this returns `None`), or something is left that isn't a document
    /// -- the failure winnow's [`Parser::parse`](winnow::Parser::parse) raises for
    /// [`crate::parse::yaml_stream`] once it returns.
    fn end_of_stream(&mut self) -> Option<Error> {
        if self.input.eof_offset() == 0 {
            self.state = State::Done;
            return None;
        }
        // No message of our own: `Parser::parse`'s trailing-input error carries a context-less
        // `ContextError` too, and the rendered position is the whole diagnostic in both cases.
        // Anything more specific would be a guess -- the offset is where the *stream* gave up,
        // which for e.g. `[a, b` is the start of the unterminated sequence, not the missing `]`.
        // An empty message renders as `crate::error`'s generic title rather than as nothing.
        Some(self.fail(&ContextError::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_document() {
        let doc = parse_document("key: value\n").unwrap();
        let Content::Map(map) = &doc.as_node().value else {
            panic!("expected a mapping, got {:?}", doc.as_node().value);
        };
        assert_eq!(map.entries()[0].value.as_str(), Some("value"));
    }

    #[test]
    fn parse_document_resolves_tags() {
        // `resolve` ran: an untagged plain `1` came back as an int, not as an unresolved scalar.
        assert_eq!(parse_document("1\n").unwrap().as_node().as_i64(), Some(1));
    }

    #[test]
    fn parse_document_reads_an_empty_stream_as_null() {
        assert!(parse_document("").unwrap().as_node().is_null());
        assert!(parse_document("# just a comment\n")
            .unwrap()
            .as_node()
            .is_null());
    }

    #[test]
    fn parse_document_rejects_a_multi_document_stream() {
        assert_eq!(
            parse_document("a: 1\n---\na: 2\n").unwrap_err(),
            Error::MultipleDocuments
        );
    }

    #[test]
    fn parse_document_reports_syntax_errors() {
        assert!(matches!(
            parse_document("[a, b").unwrap_err(),
            Error::Parse(_)
        ));
    }

    #[test]
    fn parse_document_reports_resolve_errors() {
        assert!(matches!(
            parse_document("!!int foo\n").unwrap_err(),
            Error::Resolve(_)
        ));
    }

    #[test]
    fn parse_stream_yields_every_document() {
        let docs: Vec<_> = parse_stream("1\n---\n2\n---\n3\n")
            .collect::<crate::Result<Vec<_>>>()
            .unwrap();
        let values: Vec<_> = docs.iter().map(|d| d.as_node().as_i64().unwrap()).collect();
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn parse_stream_is_empty_for_an_empty_input() {
        assert_eq!(parse_stream("").count(), 0);
        assert_eq!(parse_stream("# nothing here\n").count(), 0);
    }

    /// The point of the whole module: the first document is handed out before the rest of the
    /// input has been looked at. Parsing the stream eagerly would fail on the second document and
    /// so produce nothing at all here.
    #[test]
    fn parse_stream_yields_a_document_before_parsing_a_later_broken_one() {
        let mut docs = parse_stream("1\n---\n[unclosed");
        assert_eq!(docs.next().unwrap().unwrap().as_node().as_i64(), Some(1));
    }

    #[test]
    fn parse_stream_ends_after_a_parse_error() {
        let mut docs = parse_stream("1\n---\n[unclosed");
        assert!(docs.next().unwrap().is_ok());
        // The `---` opens a second (empty) document, and the leftover `[unclosed` then fails.
        assert!(docs.next().unwrap().unwrap().as_node().is_null());
        assert!(matches!(docs.next(), Some(Err(Error::Parse(_)))));
        assert!(docs.next().is_none());
        assert!(docs.next().is_none());
    }

    /// Spans are only worth having if they slice the right text back out, which no amount of
    /// "the node has a span" checking would catch.
    #[test]
    fn every_node_spans_the_text_it_was_parsed_from() {
        let input = "a: 1\nb: [2, 3]\n";
        let doc = parse_document(input).unwrap();
        let root = doc.as_node();
        assert_eq!(root.span().unwrap().start(), 0);

        let Content::Map(map) = &root.value else {
            panic!("expected a mapping, got {:?}", root.value);
        };
        let text = |node: &Node<'_>| &input[node.span().unwrap().range()];

        assert_eq!(text(&map.entries()[0].key), "a");
        assert_eq!(text(&map.entries()[0].value), "1");
        assert_eq!(text(&map.entries()[1].key), "b");
        assert_eq!(text(&map.entries()[1].value), "[2, 3]");

        let Content::Seq(items) = &map.entries()[1].value.value else {
            panic!("expected a sequence");
        };
        assert_eq!(text(&items[0]), "2");
        assert_eq!(text(&items[1]), "3");
    }

    /// An alias points at itself, not at the anchor it copies -- an error about *this* node should
    /// name the line it was written on.
    #[test]
    fn an_alias_spans_the_alias_not_the_anchor() {
        let input = "- &a foo\n- *a\n";
        let doc = parse_document(input).unwrap();
        let Content::Seq(items) = &doc.as_node().value else {
            panic!("expected a sequence");
        };
        // A node's span covers its properties too, `c-ns-properties` being part of the node.
        assert_eq!(&input[items[0].span().unwrap().range()], "&a foo");
        assert_eq!(&input[items[1].span().unwrap().range()], "*a");
    }

    #[test]
    fn parse_stream_continues_after_a_resolve_error() {
        let mut docs = parse_stream("!!int foo\n---\n2\n");
        assert!(matches!(docs.next(), Some(Err(Error::Resolve(_)))));
        assert_eq!(docs.next().unwrap().unwrap().as_node().as_i64(), Some(2));
        assert!(docs.next().is_none());
    }
}
