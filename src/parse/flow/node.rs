use winnow::combinator::alt;
use winnow::{combinator::opt, combinator::preceded, combinator::trace, Parser};

use crate::parse::{
    alias::alias_node, context::FlowOrKey, error::ParserError, flow::content::flow_content,
    input::InputStream, properties, spaces, spaces::IndentLevel,
};
use crate::value::{Content, Node};

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
            flow_content(context, indent_level).map(Node::unspecified),
            node_with_properties(context, indent_level, flow_content(context, indent_level)),
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
            node_with_properties(
                context,
                indent_level,
                flow_yaml_content(context, indent_level),
            ),
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
        move |input: &mut Input| {
            let start = input.checkpoint();
            let props = opt((
                properties::properties(context, indent_level),
                spaces::separate(context, indent_level),
            ))
            .parse_next(input)?
            .map(|(props, ())| props);
            let value = flow_json_content(context, indent_level).parse_next(input)?;
            let (anchor, tag) = match props {
                Some(properties::Properties { anchor, tag }) => (anchor, tag),
                None => (None, None),
            };
            properties::build_node(input, &start, anchor, tag, value)
        },
    )
}

/// Parses `c-ns-properties(n,c)` followed by optional content (`e-scalar` when absent), and
/// wires the result into a [`Node`] via [`build_node`].
///
/// Used by [`flow_node`] and [`flow_yaml_node`], which share the
/// `properties (separate content)?` shape; [`flow_json_node`] has a different shape (properties
/// are optional as a whole, but content is mandatory) so it's wired by hand instead.
fn node_with_properties<'i, Context, Input, Error, C>(
    context: Context,
    indent_level: IndentLevel,
    mut content: C,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
    C: Parser<Input, Content<'i>, Error>,
{
    trace("flow::node::node_with_properties", move |input: &mut Input| {
        let start = input.checkpoint();
        let props = properties::properties(context, indent_level).parse_next(input)?;
        let value = opt(preceded(
            spaces::separate(context, indent_level),
            content.by_ref(),
        ))
        .parse_next(input)?
        .unwrap_or(Content::Empty);
        properties::build_node(input, &start, props.anchor, props.tag, value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use winnow::error::ContextError;

    use crate::parse::{
        context::{FlowIn, FlowOut},
        input::{Input, WithAnchorStore},
        testing,
    };
    use crate::value::{Scalar, Tag};

    #[test]
    fn flow_yaml_node_with_tag_and_anchor_registers_anchor() {
        // `,` isn't ns-plain-safe in flow context, so it stops the plain scalar right after
        // "foo", leaving "rest" available to check nothing extra was consumed.
        let mut input = Input::new("!!str &a1 foo,rest");
        let mut parser = flow_yaml_node::<_, _, ContextError>(FlowIn, IndentLevel::initial());
        let node = parser.parse_next(&mut input).unwrap();
        assert_eq!(
            Node::new(
                Content::Scalar(Scalar::Plain(Cow::Borrowed("foo"))),
                Tag::Global(Cow::Borrowed("tag:yaml.org,2002:str"))
            ),
            node
        );
        assert_eq!(Some(&node), input.anchor_store().get("a1"));
    }

    #[test]
    fn flow_node_anchor_only_defaults_to_unspecified_tag() {
        let (rest, node) = testing::parse(
            flow_node(FlowIn, IndentLevel::initial()),
            "&a2 baz,rest",
        )
        .unwrap();
        assert_eq!(",rest", rest);
        assert_eq!(
            Node::unspecified(Content::Scalar(Scalar::Plain(Cow::Borrowed("baz")))),
            node
        );
    }

    #[test]
    fn flow_yaml_node_tag_only_is_empty_content() {
        let (rest, node) =
            testing::parse(flow_yaml_node(FlowOut, IndentLevel::initial()), "!!str").unwrap();
        assert_eq!("", rest);
        assert_eq!(
            Node::new(
                Content::Empty,
                Tag::Global(Cow::Borrowed("tag:yaml.org,2002:str"))
            ),
            node
        );
    }

    #[test]
    fn flow_json_node_with_tag() {
        let (rest, node) = testing::parse(
            flow_json_node(FlowOut, IndentLevel::initial()),
            "!!str \"bar\"",
        )
        .unwrap();
        assert_eq!("", rest);
        assert_eq!(
            Node::new(
                Content::Scalar(Scalar::DoubleStr(Cow::Borrowed("bar"))),
                Tag::Global(Cow::Borrowed("tag:yaml.org,2002:str"))
            ),
            node
        );
    }

    #[test]
    fn flow_node_non_specific_tag() {
        let (rest, node) =
            testing::parse(flow_node(FlowOut, IndentLevel::initial()), "! 12").unwrap();
        assert_eq!("", rest);
        assert_eq!(
            Node::new(
                Content::Scalar(Scalar::Plain(Cow::Borrowed("12"))),
                Tag::NonSpecific
            ),
            node
        );
    }

    #[test]
    fn flow_node_undeclared_handle_is_a_parse_error() {
        testing::parse(flow_node(FlowOut, IndentLevel::initial()), "!h!bar").unwrap_err();
    }
}
