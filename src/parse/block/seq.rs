use winnow::{
    combinator::{not, peek, preceded, repeat, trace},
    token::one_of,
    Parser,
};

use crate::{
    parse::{
        block::node::block_indented,
        chars,
        context::BlockIn,
        error::ParserError,
        input::InputStream,
        spaces::{self, IndentLevel},
    },
    value::Node,
};

/// Block sequence.
///
/// https://yaml.org/spec/1.2.2/#rule-l+block-sequence
#[doc(alias = "l+block-sequence")]
pub fn block_sequence<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Vec<Node<'i>>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::seq::block_sequence",
        peek(spaces::indent_at_least(indent_level + 1)).flat_map(|indent_level| {
            repeat(
                1..,
                preceded(spaces::indent(indent_level), block_seq_entry(indent_level)),
            )
        }),
    )
}

/// Block sequence entry.
///
/// https://yaml.org/spec/1.2.2/#rule-c-l-block-seq-entry
#[doc(alias = "c-l-block-seq-entry")]
pub fn block_seq_entry<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::seq::block_seq_entry",
        preceded(
            // note: `not` doesn't consume the input.
            ('-', not(one_of(chars::is_non_space))),
            block_indented(BlockIn, indent_level),
        ),
    )
}

/// Compact sequence notation, used as the content of a block sequence entry or explicit block
/// mapping key/value that itself starts with `-` on the same line (e.g. the nested `-` in
/// `- - a`).
///
/// https://yaml.org/spec/1.2.2/#rule-ns-l-compact-sequence
#[doc(alias = "ns-l-compact-sequence")]
pub fn compact_sequence<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Vec<Node<'i>>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::seq::compact_sequence", move |input: &mut Input| {
        let first = block_seq_entry(indent_level).parse_next(input)?;
        repeat(
            0..,
            preceded(spaces::indent(indent_level), block_seq_entry(indent_level)),
        )
        .fold(
            move || vec![first.clone()],
            |mut entries, entry| {
                entries.push(entry);
                entries
            },
        )
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

    /// Spec example 8.14 "Block Sequence" (the sequence value only; the enclosing mapping key
    /// belongs to Phase 4's block-mapping tests instead).
    #[test]
    fn block_sequence_example_8_14() {
        let (rest, got) =
            testing::parse(block_sequence(IndentLevel::new(0)), "  - one\n  - two\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(vec![plain("one"), plain("two")], got);
    }

    /// The nested `- - a` / `- two` shape from spec example 8.15 "Block Sequence Entry Types"'s
    /// compact-notation entry.
    #[test]
    fn compact_sequence_nested_dash() {
        let (rest, got) =
            testing::parse(compact_sequence(IndentLevel::new(2)), "- one\n  - two\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(vec![plain("one"), plain("two")], got);
    }
}
