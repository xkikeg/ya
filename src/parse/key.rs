use winnow::combinator::{opt, terminated};
use winnow::error::{StrContext, StrContextValue};
use winnow::{combinator::trace, Parser};

use crate::value::Node;

use super::{
    context::Key,
    error::ParserError,
    flow,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// YAML implicit key.
///
/// https://yaml.org/spec/1.2.2/#rule-c-s-implicit-yaml-key
#[doc(alias = "c-s-implicit-yaml-key")]
pub fn implicit_yaml_key<'i, Context, Input, Error>(
    context: Context,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: Key,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("key::implicit_yaml_key", move |input: &mut Input| {
        let start = input.checkpoint();
        // TODO: it's unclear if we should limit the length for the worst case.
        let (got, taken) = terminated(
            flow::node::flow_yaml_node(context, IndentLevel::new(0)),
            opt(spaces::separate_in_line),
        )
        .with_taken()
        .parse_next(input)?;
        if taken.chars().take(1025).count() > 1024 {
            return Err(Error::from_input(input).add_context(
                input,
                &start,
                StrContext::Expected(StrContextValue::Description(
                    "Flow YAML node with at most 1024 Unicode scalar values",
                )),
            ));
        }
        Ok(got)
    })
}

/// JSON implicit key.
///
/// https://yaml.org/spec/1.2.2/#rule-c-s-implicit-json-key
#[doc(alias = "c-s-implicit-json-key")]
pub fn implicit_json_key<'i, Context, Input, Error>(
    context: Context,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: Key,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("key::implicit_json_key", move |input: &mut Input| {
        let start = input.checkpoint();
        // TODO: it's unclear if we should limit the length for the worst case.
        let (got, taken) = terminated(
            flow::node::flow_json_node(context, IndentLevel::new(0)),
            opt(spaces::separate_in_line),
        )
        .with_taken()
        .parse_next(input)?;
        if taken.chars().take(1025).count() > 1024 {
            return Err(Error::from_input(input).add_context(
                input,
                &start,
                StrContext::Expected(StrContextValue::Description(
                    "Flow JSON node with at most 1024 Unicode scalar values",
                )),
            ));
        }
        Ok(got)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use crate::parse::{context::FlowKey, testing};
    use crate::value::{Content, Scalar};

    #[test]
    fn implicit_json_key_without_trailing_separator() {
        // c-s-implicit-json-key ends with `s-separate-in-line?` (optional):
        // a key directly followed by `:` (no whitespace) must still parse.
        let input = r#""a":b"#;
        assert_eq!(
            (
                ":b",
                Node::unspecified(Content::Scalar(Scalar::DoubleStr(Cow::Borrowed("a"))))
            ),
            testing::parse(implicit_json_key(FlowKey), input).unwrap()
        );
    }

    #[test]
    fn implicit_yaml_key_without_trailing_separator() {
        // Plain scalars only support a single character today (Phase 1 is
        // still pending), so exercise the `s-separate-in-line?` fix with a
        // one-character plain key.
        let input = "a:b";
        assert_eq!(
            (
                ":b",
                Node::unspecified(Content::Scalar(Scalar::SingleStr(Cow::Borrowed("a"))))
            ),
            testing::parse(implicit_yaml_key(FlowKey), input).unwrap()
        );
    }
}
