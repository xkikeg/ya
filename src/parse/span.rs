//! Records the input range each parsed node came from.
//!
//! Not a spec production -- the YAML grammar has nothing to say about source positions -- so
//! [`spanned`] carries no `#[doc(alias)]` and no `#rule-...` link, unlike every parser around it.
//! It exists so that errors raised *after* parsing (tag resolution, and with the `serde` feature a
//! failed `Deserialize`) can point at the text a node was written as, rather than only naming it.

use winnow::Parser;

use crate::value::{Node, Span};

use super::{error::ParserError, input::InputStream};

/// Runs `parser` and records the input range it matched on the [`Node`] it produced
/// ([`Node::span`]).
///
/// The span is recorded **only if the node doesn't already have one**. Node parsers delegate to one
/// another and frequently return the very same node (`block_node` -> `block_in_block` ->
/// `block_scalar`, say), so the innermost caller -- the one that matched the least input, and is
/// therefore the most precise -- wins, and an outer rule's leading indentation or separator never
/// widens a span a nested rule already pinned down. That makes this safe to apply to every
/// node-producing parser without reasoning about which of them nest inside which.
///
/// Takes `input` and its parser as arguments, rather than being a `parser.spanned()`-style
/// combinator, on purpose: callers invoke it from inside a `move |input: &mut Input| ...` closure
/// and build the parser they pass in *there*, so the parser's type stays out of the enclosing
/// function's `impl Parser` return type. A combinator would instead add two type layers
/// (`WithSpan`, `Map`) at every level of parser types that are already nested dozens deep, and the
/// resulting blowup makes the crate take tens of minutes to monomorphize. Same reasoning as
/// AGENT.md's note on breaking recursive-grammar cycles with a hand-rolled closure, for a
/// different symptom.
pub(super) fn spanned<'i, Input, Error, P>(
    input: &mut Input,
    mut parser: P,
) -> winnow::Result<Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
    P: Parser<Input, Node<'i>, Error>,
{
    // The same pair of positions `Parser::with_span` uses. A parser that consumes nothing (an
    // empty node) leaves them equal, giving an empty span at the position it was expected at.
    let start = input.current_token_start();
    let mut node = parser.parse_next(input)?;
    node.set_span_if_unset(Span::new(start, input.previous_token_end()));
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use winnow::combinator::{empty, preceded};

    use crate::parse::testing;
    use crate::value::{Content, Scalar};

    fn scalar(text: &str) -> Node<'_> {
        Node::unspecified(Content::Scalar(Scalar::Plain(Cow::Borrowed(text))))
    }

    #[test]
    fn records_the_matched_range() {
        let (_, node) = testing::parse(
            |input: &mut _| spanned(input, preceded("--", "abc".map(scalar))),
            "--abc",
        )
        .unwrap();
        assert_eq!(node.span(), Some(Span::new(0, 5)));
    }

    /// The whole point of the set-if-unset rule: an outer parser that also consumed a prefix must
    /// not widen the span an inner one already recorded.
    #[test]
    fn keeps_the_innermost_span() {
        let (_, node) = testing::parse(
            |input: &mut _| {
                spanned(
                    input,
                    preceded("--", |input: &mut _| spanned(input, "abc".map(scalar))),
                )
            },
            "--abc",
        )
        .unwrap();
        assert_eq!(node.span(), Some(Span::new(2, 5)));
    }

    /// A parser that consumes nothing gets an empty span at the position it was tried at -- which
    /// is what an absent mapping key or value ends up pointing at.
    #[test]
    fn empty_match_spans_nothing_at_the_current_position() {
        let (_, node) = testing::parse(
            |input: &mut _| {
                preceded("--", |input: &mut _| {
                    spanned(input, empty.value(Node::unspecified(Content::Empty)))
                })
                .parse_next(input)
            },
            "--",
        )
        .unwrap();
        assert_eq!(node.span(), Some(Span::new(2, 2)));
    }
}
