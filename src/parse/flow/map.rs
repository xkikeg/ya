use winnow::{
    combinator::{alt, delimited, empty, opt, peek, preceded, terminated, trace},
    token::one_of,
    Parser,
};

use crate::{
    parse::{
        context::{FlowOrKey, InFlow, YamlContext},
        error::ParserError,
        input::InputStream,
        spaces::{self, IndentLevel},
    },
    value::{MapEntry, Mapping, Value},
};

use super::node::{flow_json_node, flow_node, flow_yaml_node};

/// Flow mapping.
///
/// https://yaml.org/spec/1.2.2/#rule-c-flow-mapping
#[doc(alias = "c-flow-mapping")]
pub fn flow_mapping<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Mapping<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_mapping",
        delimited(
            ('{', opt(spaces::separate(context, indent_level))),
            flow_map_entries(<Context as FlowOrKey>::Flow::get(), indent_level).map(Mapping),
            '}',
        ),
    )
}

/// Flow map entries.
///
/// Modified from spec so that it may consume zero entries.
/// https://yaml.org/spec/1.2.2/#rule-ns-s-flow-map-entries
#[doc(alias = "ns-s-flow-map-entries")]
pub fn flow_map_entries<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Vec<MapEntry<'i>>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("flow::map::flow_map_entries", move |input: &mut Input| {
        let mut ret = Vec::new();
        loop {
            let elem = terminated(
                opt(flow_map_entry(context, indent_level)),
                opt(spaces::separate(context, indent_level)),
            )
            .parse_next(input)?;
            match elem {
                None => return Ok(Vec::new()),
                Some(x) => ret.push(x),
            };
            let comma = terminated(opt(','), opt(spaces::separate(context, indent_level)))
                .parse_next(input)?;
            if comma.is_none() {
                return Ok(ret);
            }
        }
    })
}

/// Flow map entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-map-entry
#[doc(alias = "ns-flow-map-entry")]
pub fn flow_map_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_entry",
        alt((
            preceded(
                ('?', spaces::separate(context, indent_level)),
                flow_map_explicit_entry(context, indent_level),
            ),
            flow_map_implicit_entry(context, indent_level),
        )),
    )
}

/// Explicit flow map entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-map-explicit-entry
#[doc(alias = "ns-flow-map-explicit-entry")]
pub fn flow_map_explicit_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_explicit_entry",
        alt((
            flow_map_implicit_entry(context, indent_level),
            empty.value(MapEntry {
                key: Value::Empty,
                value: Value::Empty,
            }),
        )),
    )
}

/// Implicit flow map entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-map-implicit-entry
#[doc(alias = "ns-flow-map-implicit-entry")]
pub fn flow_map_implicit_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_implicit_entry",
        alt((
            flow_map_yaml_key_entry(context, indent_level),
            flow_map_empty_key_entry(context, indent_level),
            flow_map_json_key_entry(context, indent_level),
        )),
    )
}

/// Yaml key flow map entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-map-yaml-key-entry
#[doc(alias = "ns-flow-map-yaml-key-entry")]
pub fn flow_map_yaml_key_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_yaml_key_entry",
        (
            flow_yaml_node(context, indent_level),
            opt(preceded(
                opt(spaces::separate(context, indent_level)),
                flow_map_separate_value(context, indent_level),
            ))
            .map(|x| x.unwrap_or(Value::Empty)),
        )
            .map(MapEntry::from_tuple),
    )
}

/// Empty key flow map entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-map-empty-key-entry
#[doc(alias = "ns-flow-map-empty-key-entry")]
pub fn flow_map_empty_key_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_empty_key_entry",
        flow_map_separate_value(context, indent_level).map(|value| MapEntry {
            key: Value::Empty,
            value,
        }),
    )
}

/// Separated map value, which means `:` requires following spaces.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-flow-map-separate-value
#[doc(alias = "c-ns-flow-map-separate-value")]
pub fn flow_map_separate_value<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Value<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_separate_value",
        preceded(
            (
                ':',
                peek(one_of(|c| !<Context as FlowOrKey>::is_plain_safe(c))),
            ),
            opt(preceded(
                spaces::separate(context, indent_level),
                flow_node(context, indent_level),
            )),
        )
        .map(|x| x.unwrap_or(Value::Empty)),
    )
}

/// Json key flow map entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-map-json-key-entry
#[doc(alias = "ns-flow-map-json-key-entry")]
pub fn flow_map_json_key_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, MapEntry<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_json_key_entry",
        (
            flow_json_node(context, indent_level),
            opt(preceded(
                opt(spaces::separate(context, indent_level)),
                flow_map_adjacent_value(context, indent_level),
            ))
            .map(|x| x.unwrap_or(Value::Empty)),
        )
            .map(MapEntry::from_tuple),
    )
}

/// Adjacent map value, which means `:` doesn't require following spaces.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-flow-map-adjacent-value
#[doc(alias = "c-ns-flow-map-adjacent-value")]
pub fn flow_map_adjacent_value<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Value<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_adjacent_value",
        preceded(
            ':',
            opt(preceded(
                opt(spaces::separate(context, indent_level)),
                flow_node(context, indent_level),
            )),
        )
        .map(|x| x.unwrap_or(Value::Empty)),
    )
}
