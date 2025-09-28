use winnow::{
    combinator::{alt, delimited, trace},
    Parser,
};

use crate::{
    parse::{
        context::{FlowOut, InOutBlock},
        error::ParserError,
        flow::node::flow_node,
        input::InputStream,
        spaces::{self, IndentLevel},
    },
    value::{Content, Node},
};

/// Block node.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+block-node
#[doc(alias = "s-l+block-node")]
pub fn block_node<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: InOutBlock,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::node::block_node",
        alt((
            block_in_block(context, indent_level),
            flow_in_block(indent_level),
        )),
    )
}

/// Block in block node.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+block-in-block
#[doc(alias = "s-l+block-in-block")]
pub fn block_in_block<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: InOutBlock,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    // TODO: implement this
    trace("block::node::block_in_block", winnow::combinator::fail)
}

/// Flow in block node.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+flow-in-block
#[doc(alias = "s-l+flow-in-block")]
pub fn flow_in_block<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::node::block_in_block",
        delimited(
            spaces::separate(FlowOut, indent_level + 1),
            flow_node(FlowOut, indent_level + 1),
            spaces::line_comments,
        ),
    )
}

/// Block indented.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+block-indented
#[doc(alias = "s-l+block-indented")]
pub fn block_indented<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Content<'i>, Error>
where
    Context: InOutBlock,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::node::block_indented",
        // TODO: implement this
        winnow::combinator::fail,
    )
}
