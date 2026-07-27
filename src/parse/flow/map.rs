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
        span::spanned,
    },
    value::{Content, MapEntry, Mapping, Node, Span},
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
                // No further entry here -- whether this is the very first iteration (a
                // genuinely empty `{}`) or after a trailing comma (`{a: 1,}`), either way it
                // means "stop, and return whatever's been collected so far", *not* "discard
                // everything collected so far": `ret` is already `Vec::new()` on the first
                // iteration, so this also correctly handles the empty-mapping case.
                None => return Ok(ret),
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
            empty.with_span().map(|((), span)| {
                // Both nodes are empty, so both span nothing, at the position the entry would
                // have started at.
                let span = Span::from(span);
                MapEntry {
                    key: Node::unspecified(Content::Empty).with_span(span),
                    value: Node::unspecified(Content::Empty).with_span(span),
                }
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
            // JSON-key tried first, matching the same ordering already established for block
            // mapping implicit keys (`block::map::block_map_implicit_key`): a JSON-shaped key
            // (a flow collection, or a quoted scalar) requires *mandatory* content, so it fails
            // outright -- and cleanly falls through to the YAML-key arm -- for anything it can't
            // parse (e.g. a plain scalar key). But `ns-flow-yaml-node`'s "properties with no
            // content" fallback (legitimate on its own, e.g. `!!str &a` alone) makes the YAML-key
            // arm *always succeed*, even on a collection-shaped key like `&a [x, &b b]`, by
            // simply stopping right after the property and leaving the collection unconsumed --
            // which is wrong, but not a parse failure `alt` can backtrack past. Trying JSON first
            // sidesteps the ambiguity entirely instead of trying to detect it after the fact.
            flow_map_json_key_entry(context, indent_level),
            flow_map_yaml_key_entry(context, indent_level),
            flow_map_empty_key_entry(context, indent_level),
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
            move |input: &mut Input| {
                spanned(
                    input,
                    opt(preceded(
                        opt(spaces::separate(context, indent_level)),
                        flow_map_separate_value(context, indent_level),
                    ))
                    .map(|x| x.unwrap_or(Node::unspecified(Content::Empty))),
                )
            },
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
        flow_map_separate_value(context, indent_level)
            .with_span()
            .map(|(value, span)| MapEntry {
                // The absent key spans nothing, at the position the `:` was found.
                key: Node::unspecified(Content::Empty).with_span(Span::new(span.start, span.start)),
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
) -> impl Parser<Input, Node<'i>, Error>
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
            move |input: &mut Input| {
                spanned(
                    input,
                    opt(preceded(
                        spaces::separate(context, indent_level),
                        flow_node(context, indent_level),
                    ))
                    .map(|x| x.unwrap_or(Node::unspecified(Content::Empty))),
                )
            },
        ),
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
            move |input: &mut Input| {
                spanned(
                    input,
                    opt(preceded(
                        opt(spaces::separate(context, indent_level)),
                        flow_map_adjacent_value(context, indent_level),
                    ))
                    .map(|x| x.unwrap_or(Node::unspecified(Content::Empty))),
                )
            },
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
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::map::flow_map_adjacent_value",
        preceded(':', move |input: &mut Input| {
            spanned(
                input,
                opt(preceded(
                    opt(spaces::separate(context, indent_level)),
                    flow_node(context, indent_level),
                ))
                .map(|x| x.unwrap_or(Node::unspecified(Content::Empty))),
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use crate::{
        parse::{context::FlowIn, testing},
        value::Scalar,
    };

    fn plain(s: &str) -> Node<'_> {
        Node::unspecified(Content::Scalar(Scalar::Plain(Cow::Borrowed(s))))
    }

    /// A trailing comma before the closing `}` used to make the whole entries loop discard
    /// *every* already-collected entry (`None => return Ok(Vec::new())`), not just stop cleanly,
    /// because "no further entry here" was conflated with "there were never any entries" --
    /// corpus case `5C5M` (spec 7.15 "Flow Mappings") is one of several that depend on this.
    #[test]
    fn trailing_comma_keeps_already_collected_entries() {
        let (rest, got) = testing::parse(
            flow_map_entries(FlowIn, IndentLevel::initial()),
            "one: two, three: four, }",
        )
        .unwrap();
        assert_eq!("}", rest);
        assert_eq!(
            vec![
                MapEntry {
                    key: plain("one"),
                    value: plain("two"),
                },
                MapEntry {
                    key: plain("three"),
                    value: plain("four"),
                },
            ],
            got
        );
    }

    #[test]
    fn empty_entries() {
        let (rest, got) =
            testing::parse(flow_map_entries(FlowIn, IndentLevel::initial()), "}").unwrap();
        assert_eq!("}", rest);
        assert_eq!(Vec::<MapEntry<'_>>::new(), got);
    }

    /// An anchor directly preceding a flow-*sequence*-shaped key (corpus case `X38W`, "Aliases in
    /// Flow Objects") used to be mis-parsed by the YAML-key arm alone: `ns-flow-yaml-node`'s
    /// legitimate "properties with no content" fallback let it stop right after the anchor,
    /// leaving the sequence unconsumed, instead of failing outright and letting the JSON-key arm
    /// (which requires content, and so correctly parses the sequence) take over. Trying the
    /// JSON-key arm first sidesteps the ambiguity.
    #[test]
    fn anchored_flow_sequence_key() {
        let (rest, got) = testing::parse(
            flow_map_entries(FlowIn, IndentLevel::initial()),
            "&a [a, &b b]: *b }",
        )
        .unwrap();
        assert_eq!("}", rest);
        assert_eq!(1, got.len());
        assert_eq!(Content::Seq(vec![plain("a"), plain("b")]), got[0].key.value);
        assert_eq!(plain("b"), got[0].value);
    }
}
