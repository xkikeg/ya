use winnow::{
    combinator::{preceded, trace},
    token::one_of,
    Parser,
};

use crate::value::{Node, Span};

use super::{error::ParserError, input::InputStream, properties::anchor_name};

/// Alias node.
///
/// The anchored node is substituted eagerly (a clone of whatever `&name` was bound to), so the
/// resulting node's *children* keep the spans they had where the anchor was defined. Its own
/// top-level span is overwritten with the alias's position instead, since that is where this
/// particular node was written -- an error about it should point at the `*name`, not at the
/// original definition somewhere else in the document.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-alias-node
#[doc(alias = "c-ns-alias-node")]
pub fn alias_node<'i, Input, Error>(input: &mut Input) -> winnow::Result<Node<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("alias::alias_node", move |input: &mut Input| {
        let start = input.checkpoint();
        let (alias, span) = preceded(one_of('*'), anchor_name)
            .with_span()
            .parse_next(input)?;
        let span = Span::from(span);
        let node = input.anchor_store().get(alias).cloned().ok_or_else(|| {
            Error::from_input(input).add_context(
                input,
                &start,
                winnow::error::StrContext::Expected(winnow::error::StrContextValue::Description(
                    "previously registered anchors",
                )),
            )
        })?;
        Ok(node.with_span(span))
    })
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use winnow::combinator::alt;

    use crate::{
        parse::{
            input::{Input, WithAnchorStore},
            testing,
        },
        value::{Content, Scalar},
    };

    #[test]
    fn alias_parses_on_registered_anchor() {
        let mut input = Input::new("*foo");
        input.anchor_store_mut().put(
            "foo".to_string(),
            Node::unspecified(Content::Scalar(Scalar::SingleStr(Cow::Borrowed("value")))),
        );

        assert_eq!(
            (
                "",
                Node::unspecified(Content::Scalar(Scalar::SingleStr(Cow::Borrowed("value"))))
            ),
            testing::parse_with_input(alias_node, input).unwrap()
        );
    }

    #[test]
    fn alias_fails_on_unregistered_anchor() {
        let mut input = Input::new("*foo");
        input.anchor_store_mut().put(
            "bar".to_string(),
            Node::unspecified(Content::Scalar(Scalar::SingleStr(Cow::Borrowed("value")))),
        );

        testing::parse_with_input(alias_node, input.clone()).unwrap_err();

        assert_eq!(
            ("foo", Node::unspecified(Content::Empty)),
            testing::parse_with_input(
                alt((
                    alias_node,
                    one_of('*').value(Node::unspecified(Content::Empty))
                )),
                input
            )
            .unwrap()
        )
    }
}
