use winnow::{
    combinator::{peek, preceded, repeat, trace},
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
    value::Content,
};

/// Block sequence.
///
/// https://yaml.org/spec/1.2.2/#rule-l+block-sequence
#[doc(alias = "l+block-sequence")]
pub fn block_sequence<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Vec<Content<'i>>, Error>
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
) -> impl Parser<Input, Content<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::seq::block_sequence",
        preceded(
            ('-', peek(one_of(chars::is_non_space))),
            block_indented(BlockIn, indent_level),
        ),
    )
}
