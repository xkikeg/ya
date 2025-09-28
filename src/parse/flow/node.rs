use winnow::combinator::alt;
use winnow::{combinator::trace, Parser};

use crate::parse::{
    alias::alias_node, context::FlowOrKey, error::ParserError, flow::content::flow_content,
    input::InputStream, spaces::IndentLevel,
};
use crate::value::Node;

use super::content::{flow_json_content, flow_yaml_content};

/// Flow node.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-node
#[doc(alias = "ns-flow-node")]
pub fn flow_node<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::node::flow_node",
        alt((
            alias_node,
            flow_content(context, indent_level).map(|v| Node::unspecified(v)),
            // TODO: fixme Support properties.
        )),
    )
}

/// Flow YAML node.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-yaml-node
#[doc(alias = "ns-flow-yaml-node")]
pub fn flow_yaml_node<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::node::flow_yaml_node",
        alt((
            alias_node,
            flow_yaml_content(context, indent_level).map(Node::unspecified),
            // TODO: fixme Support properties.
        )),
    )
}

/// Flow JSON node.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-json-node
#[doc(alias = "ns-flow-json-node")]
pub fn flow_json_node<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::node::flow_json_node",
        // TODO: fixme Support properties.
        flow_json_content(context, indent_level).map(Node::unspecified),
    )
}
