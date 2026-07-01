use winnow::{
    combinator::{alt, trace},
    Parser,
};

use crate::{
    parse::{
        context::FlowOrKey,
        double::double_quoted,
        error::ParserError,
        flow::{map::flow_mapping, seq::flow_sequence},
        input::InputStream,
        plain::plain,
        scalar::single_quoted,
        spaces::IndentLevel,
    },
    value::{Content, Scalar},
};

/// Flow content.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-content
#[doc(alias = "ns-flow-content")]
pub fn flow_content<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Content<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::content::flow_content",
        alt((
            flow_yaml_content(context, indent_level),
            flow_json_content(context, indent_level),
        )),
    )
}

/// Flow YAML content.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-yaml-content
#[doc(alias = "ns-flow-yaml-content")]
pub fn flow_yaml_content<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Content<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::content::flow_yaml_content",
        plain(context, indent_level).map(Content::Scalar),
    )
}

/// Flow JSON-style content.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-json-content
#[doc(alias = "ns-flow-json-content")]
pub fn flow_json_content<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Content<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::content::flow_json_content",
        alt((
            flow_sequence(context, indent_level).map(Content::Seq),
            flow_mapping(context, indent_level).map(Content::Map),
            single_quoted(context, indent_level).map(|s| Content::Scalar(Scalar::SingleStr(s))),
            double_quoted(context, indent_level).map(|s| Content::Scalar(Scalar::DoubleStr(s))),
        )),
    )
}
