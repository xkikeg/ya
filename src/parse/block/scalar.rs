use winnow::{
    combinator::{preceded, trace},
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
        "block::content::block_scalar",
        preceded(
            spaces::separate(context, indent_level + 1),
            // TODO fixme support properties
            // (c-ns-properties(n+1, c) s-separate(n+1,c))?
            // TODO implement this
            winnow::combinator::fail,
        ),
    )
}
