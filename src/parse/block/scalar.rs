use winnow::{
    combinator::{alt, opt, trace},
    Parser,
};

use crate::{
    parse::{
        context::InOutBlock,
        error::ParserError,
        input::InputStream,
        properties,
        spaces::{self, IndentLevel},
    },
    value::{Content, Node, Scalar},
};

use super::{folded, literal};

/// Block scalar.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+block-scalar
#[doc(alias = "s-l+block-scalar")]
pub fn block_scalar<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: InOutBlock,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::scalar::block_scalar", move |input: &mut Input| {
        spaces::separate(context, indent_level + 1).parse_next(input)?;
        let start = input.checkpoint();
        let props = opt((
            properties::properties(context, indent_level + 1),
            spaces::separate(context, indent_level + 1),
        ))
        .parse_next(input)?
        .map(|(props, ())| props);
        let scalar = alt((
            literal::literal(indent_level).map(Scalar::Literal),
            folded::folded(indent_level).map(Scalar::Folded),
        ))
        .parse_next(input)?;
        let (anchor, tag) = match props {
            Some(properties::Properties { anchor, tag }) => (anchor, tag),
            None => (None, None),
        };
        properties::build_node(input, &start, anchor, tag, Content::Scalar(scalar))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use crate::{
        parse::{context::BlockOut, testing},
        value::Tag,
    };

    #[test]
    fn block_scalar_literal() {
        let (rest, got) =
            testing::parse(block_scalar(BlockOut, IndentLevel::initial()), " |\n  text\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(
            Node::unspecified(Content::Scalar(Scalar::Literal(Cow::Owned(
                "text\n".to_string()
            )))),
            got
        );
    }

    #[test]
    fn block_scalar_folded() {
        let (rest, got) =
            testing::parse(block_scalar(BlockOut, IndentLevel::initial()), " >\n  text\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(
            Node::unspecified(Content::Scalar(Scalar::Folded(Cow::Owned(
                "text\n".to_string()
            )))),
            got
        );
    }

    #[test]
    fn block_scalar_with_properties() {
        let (rest, got) = testing::parse(
            block_scalar(BlockOut, IndentLevel::initial()),
            " !!str &a1 |\n  text\n",
        )
        .unwrap();
        assert_eq!("", rest);
        assert_eq!(
            Node::new(
                Content::Scalar(Scalar::Literal(Cow::Owned("text\n".to_string()))),
                Tag::Global(Cow::Borrowed("tag:yaml.org,2002:str"))
            ),
            got
        );
    }
}
