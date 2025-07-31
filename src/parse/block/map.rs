use winnow::{
    combinator::{peek, preceded, repeat, trace},
    Parser,
};

use crate::{
    parse::{
        error::ParserError,
        input::InputStream,
        spaces::{self, IndentLevel},
    },
    value::{MapEntry, Mapping},
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
        // TODO fixme implement this
        winnow::combinator::fail,
    )
}
