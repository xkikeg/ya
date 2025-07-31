use winnow::combinator::{alt, preceded};
use winnow::{combinator::trace, Parser};

use crate::parse::{
    context::{FlowKey, InFlow},
    error::ParserError,
    flow,
    input::InputStream,
    key,
    spaces::{self, IndentLevel},
};
use crate::value::MapEntry;

/// Flow pair.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-pair
#[doc(alias = "ns-flow-pair")]
pub fn flow_pair<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::flow_pair",
        alt((
            preceded(
                ('?', spaces::separate(context, indent_level)),
                flow::map::flow_map_explicit_entry(context, indent_level),
            ),
            flow_pair_entry(context, indent_level),
        )),
    )
}

/// Flow pair entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-pair-entry
#[doc(alias = "ns-flow-pair-entry")]
pub fn flow_pair_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::flow_pair_entry",
        alt((
            flow_pair_yaml_key_entry(context, indent_level),
            flow::map::flow_map_empty_key_entry(context, indent_level),
            flow_pair_json_key_entry(context, indent_level),
        )),
    )
}

/// Yaml key flow pair entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-pair-yaml-key-entry
#[doc(alias = "ns-flow-pair-yaml-key-entry")]
pub fn flow_pair_yaml_key_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::flow_pair_yaml_key_entry",
        (
            key::implicit_yaml_key(FlowKey),
            flow::map::flow_map_separate_value(context, indent_level),
        )
            .map(MapEntry::from_tuple),
    )
}

/// Json key flow pair entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-pair-json-key-entry
#[doc(alias = "ns-flow-pair-json-key-entry")]
pub fn flow_pair_json_key_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::flow_pair_json_key_entry",
        (
            key::implicit_json_key(FlowKey),
            flow::map::flow_map_adjacent_value(context, indent_level),
        )
            .map(MapEntry::from_tuple),
    )
}
