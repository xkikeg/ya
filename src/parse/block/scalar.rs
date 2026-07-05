use winnow::{
    combinator::{alt, preceded, trace},
    Parser,
};

use crate::{
    parse::{
        context::InOutBlock,
        error::ParserError,
        input::InputStream,
        spaces::{self, IndentLevel},
    },
    value::Scalar,
};

use super::{folded, literal};

/// Block scalar.
///
/// https://yaml.org/spec/1.2.2/#rule-s-l+block-scalar
#[doc(alias = "s-l+block-scalar")]
pub fn block_scalar<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Scalar<'i>, Error>
where
    Context: InOutBlock,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "block::scalar::block_scalar",
        preceded(
            spaces::separate(context, indent_level + 1),
            // TODO(Phase 4): properties (c-ns-properties(n+1,c) s-separate(n+1,c))? here. Wiring
            // them in requires `block_scalar` to return a `Node` (carrying the anchor/tag)
            // instead of a bare `Scalar` -- that signature change is bundled into Phase 4a's
            // wider Content -> Node migration for block constructs, so it's deferred there
            // rather than done piecemeal here.
            alt((
                literal::literal(indent_level).map(Scalar::Literal),
                folded::folded(indent_level).map(Scalar::Folded),
            )),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use crate::parse::{context::BlockOut, testing};

    #[test]
    fn block_scalar_literal() {
        let (rest, got) =
            testing::parse(block_scalar(BlockOut, IndentLevel::initial()), " |\n  text\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(Scalar::Literal(Cow::Owned("text\n".to_string())), got);
    }

    #[test]
    fn block_scalar_folded() {
        let (rest, got) =
            testing::parse(block_scalar(BlockOut, IndentLevel::initial()), " >\n  text\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(Scalar::Folded(Cow::Owned("text\n".to_string())), got);
    }
}
