//! Node properties: anchor and tag.
//!
//! https://yaml.org/spec/1.2.2/#69-node-properties

use std::borrow::Cow;

use winnow::{
    combinator::{alt, delimited, opt, preceded, repeat, trace},
    error::{StrContext, StrContextValue},
    stream::Stream,
    token::{one_of, take_while},
    Parser,
};

use crate::value::{self, Content, Node};

use super::{
    chars,
    context::YamlContext,
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
    tag_handles::TagHandles,
};

/// The two optional node properties, in whichever order they were written.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-properties
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Properties<'i> {
    pub anchor: Option<&'i str>,
    pub tag: Option<TagProperty<'i>>,
}

/// The tag property, before being resolved against the tag-handle map (see [`resolve_tag`]).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum TagProperty<'i> {
    Verbatim(&'i str),
    Shorthand { handle: &'i str, suffix: &'i str },
    NonSpecific,
}

/// Node properties: a tag property optionally followed by an anchor property, or an anchor
/// property optionally followed by a tag property.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-properties
#[doc(alias = "c-ns-properties")]
pub(super) fn properties<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Properties<'i>, Error>
where
    Context: YamlContext,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::properties",
        alt((
            (
                tag_property,
                opt(preceded(
                    spaces::separate(context, indent_level),
                    anchor_property,
                )),
            )
                .map(|(tag, anchor)| Properties {
                    anchor,
                    tag: Some(tag),
                }),
            (
                anchor_property,
                opt(preceded(
                    spaces::separate(context, indent_level),
                    tag_property,
                )),
            )
                .map(|(anchor, tag)| Properties {
                    anchor: Some(anchor),
                    tag,
                }),
        )),
    )
}

/// Tag property: a verbatim tag, a shorthand tag, or the non-specific tag.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-tag-property
#[doc(alias = "c-ns-tag-property")]
pub(super) fn tag_property<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<TagProperty<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::tag_property",
        // Order matters: shorthand before non-specific, since a handle-only prefix like `!e!`
        // must get a chance to find its (required) suffix before falling back to bare `!`.
        alt((verbatim_tag, shorthand_tag, non_specific_tag)),
    )
    .parse_next(input)
}

/// Verbatim tag: `!<...>`, delivered as-is, not subject to tag resolution.
///
/// https://yaml.org/spec/1.2.2/#rule-c-verbatim-tag
#[doc(alias = "c-verbatim-tag")]
fn verbatim_tag<'i, Input, Error>(input: &mut Input) -> winnow::Result<TagProperty<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::verbatim_tag",
        delimited("!<", chars::uri_chars, '>')
            // Spec prose (example 6.25): verbatim tags aren't resolved, so a lone `!` (the
            // non-specific tag marker) is not a meaningful verbatim tag; it must be written as
            // bare `!` (`c-non-specific-tag`) instead. Full URI-vs-local-tag validity beyond that
            // (e.g. rejecting `!<$:?>`) needs real URI-syntax validation, which is out of scope
            // here (deferred, like the rest of tag *resolution*, to AGENT.md Phase 6).
            .verify(|s: &&str| *s != "!")
            .map(TagProperty::Verbatim),
    )
    .parse_next(input)
}

/// Shorthand tag: a tag handle followed by a non-empty suffix.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-shorthand-tag
#[doc(alias = "c-ns-shorthand-tag")]
fn shorthand_tag<'i, Input, Error>(input: &mut Input) -> winnow::Result<TagProperty<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::shorthand_tag",
        (tag_handle, tag_chars).map(|(handle, suffix)| TagProperty::Shorthand { handle, suffix }),
    )
    .parse_next(input)
}

/// Non-specific tag: a bare `!`, forcing str/map/seq at resolution instead of allowing
/// plain-scalar schema resolution.
///
/// https://yaml.org/spec/1.2.2/#rule-c-non-specific-tag
#[doc(alias = "c-non-specific-tag")]
fn non_specific_tag<'i, Input, Error>(input: &mut Input) -> winnow::Result<TagProperty<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::non_specific_tag",
        '!'.value(TagProperty::NonSpecific),
    )
    .parse_next(input)
}

/// Tag handle: the primary (`!`), secondary (`!!`), or a named (`!foo!`) handle.
///
/// Shared with [`super::directive::tag_directive`], which parses the same rule for `%TAG`.
///
/// https://yaml.org/spec/1.2.2/#rule-c-tag-handle
#[doc(alias = "c-tag-handle")]
pub(super) fn tag_handle<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::tag_handle",
        // Named before secondary before primary: each is a prefix of the next's possible input
        // (`!`), so the more specific alternatives must be tried first.
        alt((named_tag_handle, "!!", "!")),
    )
    .parse_next(input)
}

/// Named tag handle: `!` + word chars + `!`.
///
/// https://yaml.org/spec/1.2.2/#rule-c-named-tag-handle
#[doc(alias = "c-named-tag-handle")]
fn named_tag_handle<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::named_tag_handle",
        ('!', take_while(1.., chars::is_word_char), '!').take(),
    )
    .parse_next(input)
}

/// Tag chars (`ns-tag-char+`): like [`chars::uri_chars`], a slice parser rather than a character
/// predicate, since `%` must be followed by exactly two hex digits.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-tag-char
#[doc(alias = "ns-tag-char")]
fn tag_chars<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::tag_chars",
        repeat(
            1..,
            alt((
                one_of(chars::is_tag_char).void(),
                (
                    '%',
                    one_of(|c: char| c.is_ascii_hexdigit()),
                    one_of(|c: char| c.is_ascii_hexdigit()),
                )
                    .void(),
            )),
        )
        .map(|()| ())
        .take(),
    )
    .parse_next(input)
}

/// Anchor property: `&` + anchor name.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-anchor-property
#[doc(alias = "c-ns-anchor-property")]
pub(super) fn anchor_property<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("properties::anchor_property", preceded('&', anchor_name)).parse_next(input)
}

/// Anchor name: one or more non-space chars, excluding flow indicators.
///
/// Shared with [`super::alias::alias_node`], which parses the same rule for `*name`.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-anchor-name
#[doc(alias = "ns-anchor-name")]
pub(super) fn anchor_name<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "properties::anchor_name",
        take_while(1.., |c| {
            chars::is_non_space(c) && !chars::is_flow_indicator(c)
        }),
    )
    .parse_next(input)
}

/// Resolves a parsed [`TagProperty`] against the tag-handle map into a [`value::Tag`].
///
/// Returns `None` when a shorthand's handle was never declared (no default, and no matching
/// `%TAG` directive) -- the only failure mode, which the caller must turn into a parse error.
/// Mapping well-known `tag:yaml.org,2002:*` URIs to [`value::StandardTag`] is deferred to
/// AGENT.md Phase 6 (Core Schema); for now every non-non-specific tag becomes `Tag::Global`.
pub(super) fn resolve_tag<'i>(
    tag_handles: &TagHandles<'i>,
    prop: TagProperty<'i>,
) -> Option<value::Tag<'i>> {
    match prop {
        TagProperty::Verbatim(s) => Some(value::Tag::Global(Cow::Borrowed(s))),
        TagProperty::NonSpecific => Some(value::Tag::NonSpecific),
        TagProperty::Shorthand { handle, suffix } => {
            let prefix = tag_handles.get(handle)?;
            Some(value::Tag::Global(Cow::Owned(format!("{prefix}{suffix}"))))
        }
    }
}

/// Resolves the (optional) tag property against the parse-time tag-handle map, builds the
/// `Node`, and registers its anchor (if any) so later aliases can resolve it.
///
/// Shared by every node-constructing rule that carries `c-ns-properties` (flow and block nodes
/// alike): [`super::flow::node::flow_node`] & co., and block collections/scalars.
pub(super) fn build_node<'i, Input, Error>(
    input: &mut Input,
    start: &<Input as Stream>::Checkpoint,
    anchor: Option<&'i str>,
    tag: Option<TagProperty<'i>>,
    content: Content<'i>,
) -> winnow::Result<Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    let tag = match tag {
        None => value::Tag::Unspecified,
        Some(prop) => resolve_tag(input.tag_handles(), prop).ok_or_else(|| {
            Error::from_input(input).add_context(
                input,
                start,
                StrContext::Expected(StrContextValue::Description("a declared tag handle")),
            )
        })?,
    };
    let node = Node::new(content, tag);
    if let Some(name) = anchor {
        input.anchor_store_mut().put(name.to_string(), node.clone());
    }
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::{context::FlowOut, testing};

    #[test]
    fn tag_then_anchor() {
        let (rest, props) = testing::parse(
            properties(FlowOut, IndentLevel::initial()),
            "!!str &a1 rest",
        )
        .unwrap();
        assert_eq!(" rest", rest);
        assert_eq!(Some("a1"), props.anchor);
        assert_eq!(
            Some(TagProperty::Shorthand {
                handle: "!!",
                suffix: "str"
            }),
            props.tag
        );
    }

    #[test]
    fn anchor_then_tag() {
        let (rest, props) = testing::parse(
            properties(FlowOut, IndentLevel::initial()),
            "&a2 !!str rest",
        )
        .unwrap();
        assert_eq!(" rest", rest);
        assert_eq!(Some("a2"), props.anchor);
        assert_eq!(
            Some(TagProperty::Shorthand {
                handle: "!!",
                suffix: "str"
            }),
            props.tag
        );
    }

    #[test]
    fn anchor_only() {
        let (rest, props) =
            testing::parse(properties(FlowOut, IndentLevel::initial()), "&a1 rest").unwrap();
        assert_eq!(" rest", rest);
        assert_eq!(Some("a1"), props.anchor);
        assert_eq!(None, props.tag);
    }

    #[test]
    fn verbatim_tag_global() {
        assert_eq!(
            ("", TagProperty::Verbatim("tag:yaml.org,2002:str")),
            testing::parse(verbatim_tag, "!<tag:yaml.org,2002:str>").unwrap()
        );
    }

    #[test]
    fn verbatim_tag_local() {
        assert_eq!(
            ("", TagProperty::Verbatim("!bar")),
            testing::parse(verbatim_tag, "!<!bar>").unwrap()
        );
    }

    #[test]
    fn verbatim_tag_rejects_bare_non_specific() {
        testing::parse(verbatim_tag, "!<!>").unwrap_err();
    }

    #[test]
    fn shorthand_primary() {
        assert_eq!(
            (
                "",
                TagProperty::Shorthand {
                    handle: "!",
                    suffix: "local"
                }
            ),
            testing::parse(shorthand_tag, "!local").unwrap()
        );
    }

    #[test]
    fn shorthand_secondary() {
        assert_eq!(
            (
                "",
                TagProperty::Shorthand {
                    handle: "!!",
                    suffix: "str"
                }
            ),
            testing::parse(shorthand_tag, "!!str").unwrap()
        );
    }

    #[test]
    fn shorthand_named_with_percent_escape() {
        assert_eq!(
            (
                "",
                TagProperty::Shorthand {
                    handle: "!e!",
                    suffix: "tag%21"
                }
            ),
            testing::parse(shorthand_tag, "!e!tag%21").unwrap()
        );
    }

    #[test]
    fn shorthand_requires_non_empty_suffix() {
        testing::parse(shorthand_tag, "!e!").unwrap_err();
    }

    #[test]
    fn non_specific_tag_bare() {
        assert_eq!(
            ("", TagProperty::NonSpecific),
            testing::parse(non_specific_tag, "!").unwrap()
        );
    }

    #[test]
    fn resolve_tag_shorthand_uses_default_handles() {
        let handles = TagHandles::new();
        assert_eq!(
            Some(value::Tag::Global(Cow::Borrowed("tag:yaml.org,2002:str"))),
            resolve_tag(
                &handles,
                TagProperty::Shorthand {
                    handle: "!!",
                    suffix: "str"
                }
            )
        );
    }

    #[test]
    fn resolve_tag_shorthand_undeclared_handle_fails() {
        let handles = TagHandles::new();
        assert_eq!(
            None,
            resolve_tag(
                &handles,
                TagProperty::Shorthand {
                    handle: "!h!",
                    suffix: "bar"
                }
            )
        );
    }
}
