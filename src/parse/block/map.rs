use winnow::{
    combinator::{alt, not, peek, preceded, repeat, terminated, trace},
    token::one_of,
    Parser,
};

use crate::{
    parse::{
        block::node::{self, block_indented},
        chars,
        context::{BlockKey, BlockOut},
        error::ParserError,
        input::InputStream,
        key,
        spaces::{self, IndentLevel},
    },
    value::{MapEntry, Mapping, Node},
};

/// Block mapping.
///
/// https://yaml.org/spec/1.2.2/#rule-l+block-mapping
#[doc(alias = "l+block-mapping")]
pub fn block_mapping<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Mapping<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_mapping",
        peek(spaces::indent_at_least(indent_level + 1)).flat_map(|indent_level| {
            repeat(
                1..,
                preceded(spaces::indent(indent_level), block_map_entry(indent_level)),
            )
            .map(Mapping)
        }),
    )
}

/// Block map entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-l-block-map-entry
#[doc(alias = "ns-l-block-map-entry")]
pub fn block_map_entry<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_map_entry",
        alt((
            block_map_explicit_entry(indent_level),
            block_map_implicit_entry(indent_level),
        )),
    )
}

/// Explicit block mapping entry: `? key` optionally followed by `: value`.
///
/// https://yaml.org/spec/1.2.2/#rule-c-l-block-map-explicit-entry
#[doc(alias = "c-l-block-map-explicit-entry")]
fn block_map_explicit_entry<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_map_explicit_entry",
        (
            block_map_explicit_key(indent_level),
            alt((block_map_explicit_value(indent_level), node::e_node)),
        )
            .map(MapEntry::from_tuple),
    )
}

/// Explicit block mapping key: `?` (not followed by a non-whitespace char, matching the spec's
/// own annotation on `c-mapping-key` here -- otherwise it's a plain scalar starting with `?`,
/// e.g. `?foo`, not an explicit-key marker; the same lookahead hazard Phase 0 already fixed for
/// `c-l-block-seq-entry`'s `-`) followed by an indented node.
///
/// https://yaml.org/spec/1.2.2/#rule-c-l-block-map-explicit-key
#[doc(alias = "c-l-block-map-explicit-key")]
fn block_map_explicit_key<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_map_explicit_key",
        preceded(
            ('?', not(one_of(chars::is_non_space))),
            block_indented(BlockOut, indent_level),
        ),
    )
}

/// Explicit block mapping value: `s-indent(n) ":" <indented node>`.
///
/// https://yaml.org/spec/1.2.2/#rule-l-block-map-explicit-value
#[doc(alias = "l-block-map-explicit-value")]
fn block_map_explicit_value<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_map_explicit_value",
        preceded(
            (spaces::indent(indent_level), ':'),
            block_indented(BlockOut, indent_level),
        ),
    )
}

/// Implicit block mapping entry: an (optionally empty) key immediately followed by its value.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-l-block-map-implicit-entry
#[doc(alias = "ns-l-block-map-implicit-entry")]
fn block_map_implicit_entry<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_map_implicit_entry",
        (
            alt((block_map_implicit_key, node::e_node)),
            block_map_implicit_value(indent_level),
        )
            .map(MapEntry::from_tuple),
    )
}

/// Implicit block mapping key: a JSON- or YAML-style implicit key, in block-key context.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-s-block-map-implicit-key
#[doc(alias = "ns-s-block-map-implicit-key")]
fn block_map_implicit_key<'i, Input, Error>(input: &mut Input) -> winnow::Result<Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_map_implicit_key",
        alt((
            key::implicit_json_key(BlockKey),
            key::implicit_yaml_key(BlockKey),
        )),
    )
    .parse_next(input)
}

/// Implicit block mapping value: `":" ( s-l+block-node(n,BLOCK-OUT) | ( e-node s-l-comments ) )`.
///
/// Hand-rolled as a closure (rather than a plain combinator chain) so the call into
/// [`node::block_node`] is deferred to parse time. This function sits on a recursion cycle back
/// to `block_node` (`block_node` -> `block_in_block` -> `block_collection` -> `block_mapping` ->
/// `block_map_entry` -> `block_map_implicit_entry` -> here) that never passes through
/// `block_indented`'s own cycle-breaking closure, so it needs its own: see AGENT.md's note on
/// breaking recursive-grammar cycles ("Recursive grammar rules must break the construction cycle
/// with a hand-rolled closure").
///
/// https://yaml.org/spec/1.2.2/#rule-c-l-block-map-implicit-value
#[doc(alias = "c-l-block-map-implicit-value")]
fn block_map_implicit_value<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::map::block_map_implicit_value",
        move |input: &mut Input| {
            preceded(
                ':',
                alt((
                    node::block_node(BlockOut, indent_level),
                    terminated(node::e_node, spaces::line_comments),
                )),
            )
            .parse_next(input)
        },
    )
}

/// Compact mapping notation, used as the content of a block sequence entry that itself starts a
/// mapping on the same line (e.g. `- key: value`).
///
/// https://yaml.org/spec/1.2.2/#rule-ns-l-compact-mapping
#[doc(alias = "ns-l-compact-mapping")]
pub fn compact_mapping<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Mapping<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::map::compact_mapping", move |input: &mut Input| {
        let first = block_map_entry(indent_level).parse_next(input)?;
        repeat(
            0..,
            preceded(spaces::indent(indent_level), block_map_entry(indent_level)),
        )
        .fold(
            move || vec![first.clone()],
            |mut entries, entry| {
                entries.push(entry);
                entries
            },
        )
        .map(Mapping)
        .parse_next(input)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use crate::{
        parse::testing,
        value::{Content, Scalar},
    };

    fn plain(s: &str) -> Node<'_> {
        Node::unspecified(Content::Scalar(Scalar::Plain(Cow::Borrowed(s))))
    }

    fn entry<'i>(key: Node<'i>, value: Node<'i>) -> MapEntry<'i> {
        MapEntry { key, value }
    }

    /// Spec example 8.16 "Block Mappings" (the mapping value only).
    #[test]
    fn block_mapping_example_8_16() {
        let (rest, got) =
            testing::parse(block_mapping(IndentLevel::new(0)), " key: value\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(Mapping(vec![entry(plain("key"), plain("value"))]), got);
    }

    /// Spec example 8.17 "Explicit Block Mapping Entries", first (single-line) entry only:
    /// `? explicit key\n: value\n`.
    #[test]
    fn block_map_explicit_entry_example_8_17() {
        let (rest, got) = testing::parse(
            block_map_explicit_entry(IndentLevel::initial()),
            "? explicit key\n: value\n",
        )
        .unwrap();
        assert_eq!("", rest);
        assert_eq!(entry(plain("explicit key"), plain("value")), got);
    }

    /// A `? key` with no following `:` value line at all resolves the value to the empty node.
    #[test]
    fn block_map_explicit_entry_without_value_is_empty() {
        let (rest, got) =
            testing::parse(block_map_explicit_entry(IndentLevel::initial()), "? key\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(entry(plain("key"), Node::unspecified(Content::Empty)), got);
    }

    /// Spec example 8.19 "Compact Block Mappings": `- sun: yellow` -- the compact-mapping content
    /// of a block sequence entry.
    #[test]
    fn compact_mapping_example_8_19() {
        let (rest, got) =
            testing::parse(compact_mapping(IndentLevel::new(2)), "sun: yellow\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(Mapping(vec![entry(plain("sun"), plain("yellow"))]), got);
    }
}
