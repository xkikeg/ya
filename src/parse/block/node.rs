use winnow::{
    combinator::{alt, delimited, empty, terminated, trace},
    token::take_while,
    Parser,
};

use crate::{
    parse::{
        context::{FlowOut, InOutBlock},
        error::ParserError,
        flow::node::flow_node,
        input::InputStream,
        properties,
        spaces::{self, IndentLevel},
        span::spanned,
    },
    value::{Content, Node},
};

use super::{map, scalar, seq};

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
    // `spanned` here is a backstop rather than the precise source: both arms bottom out in
    // parsers that record their own (narrower) span, and set-if-unset keeps those. It only takes
    // effect if some path ever produces a node nothing else spanned.
    trace("block::node::block_node", move |input: &mut Input| {
        spanned(
            input,
            alt((
                block_in_block(context, indent_level),
                flow_in_block(indent_level),
            )),
        )
    })
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
    trace(
        "block::node::block_in_block",
        alt((
            scalar::block_scalar(context, indent_level),
            block_collection(context, indent_level),
        )),
    )
}

/// Block collection: an optionally tagged/anchored block sequence or block mapping.
///
/// Hand-rolled as a closure: besides needing imperative access to `input` to register the
/// optional anchor (see [`properties::build_node`]), this is one of the two places (along with
/// [`block_indented`]) where a call back into the block-node recursion cycle must be deferred to
/// parse time rather than embedded eagerly in this function's own opaque return type -- here, the
/// cycle is `block_node` -> `block_in_block` -> `block_collection` -> `block_mapping` ->
/// `block_map_entry` -> ... -> `block_map_implicit_value` -> `block_node` again, which never
/// passes through [`block_indented`]'s own cycle-breaking closure. See AGENT.md's note on
/// breaking recursive-grammar cycles with a hand-rolled closure.
///
/// The leading `( s-separate(n+1,c) c-ns-properties(n+1,c) )?` group is only a valid *collection*
/// property when it's immediately followed by `s-l-comments` -- i.e. it stands alone on its own
/// line, with nothing else before the collection's own (indented) entries start. If a node
/// property is present but *doesn't* stand alone (e.g. `&a a: b`, where `&a` anchors the mapping's
/// first implicit key, not the mapping itself), `s-l-comments` fails, and the whole
/// properties-bearing alternative must backtrack rather than hard-fail: the two are tried as
/// genuine alternatives (`alt`), not as an `opt(...)` group whose success is locked in before its
/// consequences (the trailing `s-l-comments`) are known.
///
/// **A second, deeper instance of that same commit-too-early hazard lives *inside*
/// [`properties::properties`] itself** (yaml-test-suite case `9KAX`'s last document, `!!map\n&a8
/// !!str key8: value7`): its own `(tag_property (s-separate(n,c) anchor_property)?)` allows the
/// optional trailing anchor/tag to cross a line break freely, with no way to know from inside
/// `properties()` that the anchor on the next line (`&a8`) actually belongs to a completely
/// different, later node (the nested mapping's first key) rather than completing *this*
/// collection's own property group. Once `properties()` returns `Ok` having greedily consumed
/// both, that choice is locked in -- `alt`/`opt` only backtrack into their own listed arms, never
/// back into an already-`Ok` sub-parse just because something *later* (here, this same
/// `s-l-comments` check) fails. So the fix is the same shape one level down: enumerate the
/// narrower interpretations (tag-only, anchor-only) as explicit fallback arms *at this call site*
/// -- where `s-l-comments` is checked -- rather than trying to make `properties()` itself smarter
/// about what follows it (it has no way to know; only this caller does).
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+block-collection
#[doc(alias = "s-l+block-collection")]
fn block_collection<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: InOutBlock,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::node::block_collection", move |input: &mut Input| {
        spanned(input, move |input: &mut Input| {
            let start = input.checkpoint();
            let props = alt((
                delimited(
                    spaces::separate(context, indent_level + 1),
                    properties::properties(context, indent_level + 1),
                    spaces::line_comments,
                )
                .map(Some),
                delimited(
                    spaces::separate(context, indent_level + 1),
                    properties::tag_property.map(|tag| properties::Properties {
                        anchor: None,
                        tag: Some(tag),
                    }),
                    spaces::line_comments,
                )
                .map(Some),
                delimited(
                    spaces::separate(context, indent_level + 1),
                    properties::anchor_property.map(|anchor| properties::Properties {
                        anchor: Some(anchor),
                        tag: None,
                    }),
                    spaces::line_comments,
                )
                .map(Some),
                spaces::line_comments.value(None),
            ))
            .parse_next(input)?;
            let content = alt((
                seq::block_sequence(Context::seq_space(indent_level)).map(Content::Seq),
                map::block_mapping(indent_level).map(Content::Map),
            ))
            .parse_next(input)?;
            let (anchor, tag) = match props {
                Some(properties::Properties { anchor, tag }) => (anchor, tag),
                None => (None, None),
            };
            properties::build_node(input, &start, anchor, tag, content)
        })
    })
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
        "block::node::flow_in_block",
        delimited(
            spaces::separate(FlowOut, indent_level + 1),
            flow_node(FlowOut, indent_level + 1),
            spaces::line_comments,
        ),
    )
}

/// The empty node (`e-node`): no content, no properties.
///
/// https://yaml.org/spec/1.2.2/#rule-e-node
#[doc(alias = "e-node")]
pub(super) fn e_node<'i, Input, Error>(input: &mut Input) -> winnow::Result<Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::node::e_node", |input: &mut Input| {
        spanned(input, empty.value(Node::unspecified(Content::Empty)))
    })
    .parse_next(input)
}

/// Block indented.
///
/// Hand-rolled as a closure: `block_indented` is directly self-recursive (`block_indented` ->
/// [`compact_notation`] -> `compact_sequence`/`compact_mapping` -> `block_seq_entry`/
/// `block_map_entry` -> `block_indented` again, for the compact-notation case `- - a` / `- a: b`),
/// so its call into `compact_notation` (and `block_node`) must be deferred to parse time rather
/// than embedded eagerly in this function's own opaque return type -- see AGENT.md's note on
/// breaking recursive-grammar cycles with a hand-rolled closure.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+block-indented
#[doc(alias = "s-l+block-indented")]
pub fn block_indented<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: InOutBlock,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::node::block_indented", move |input: &mut Input| {
        alt((
            compact_notation(indent_level),
            block_node(context, indent_level),
            terminated(e_node, spaces::line_comments),
        ))
        .parse_next(input)
    })
}

/// Compact-notation arm of [`block_indented`]: `s-indent(m) ( ns-l-compact-sequence(n+1+m) |
/// ns-l-compact-mapping(n+1+m) )`, for whatever `m >= 0` spaces precede the nested collection on
/// the same line (e.g. the second `-` in `- - a`, or the key in `- a: b`). Greedily consuming the
/// maximal run of spaces is correct here since compact-notation content never itself starts with
/// further leading space -- whatever precedes it *is* `m`.
fn compact_notation<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::node::compact_notation", move |input: &mut Input| {
        let m = take_while(0.., b' ').parse_next(input)?.len();
        let indent_level = indent_level + (m + 1);
        // Spanned *after* the `m` leading spaces, so the collection's span starts at its first
        // entry rather than at the whitespace separating it from the `-`/key that introduced it.
        spanned(
            input,
            alt((
                seq::compact_sequence(indent_level)
                    .map(|entries| Node::unspecified(Content::Seq(entries))),
                map::compact_mapping(indent_level)
                    .map(|mapping| Node::unspecified(Content::Map(mapping))),
            )),
        )
    })
}
