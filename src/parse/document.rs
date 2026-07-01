use winnow::{
    combinator::{alt, dispatch, opt, peek, preceded, repeat, trace},
    token::any,
    Parser,
};

use crate::value::{self, Document};

use super::{
    block, chars,
    context::BlockIn,
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// Stream of documents.
///
/// https://yaml.org/spec/1.2.2/#rule-l-yaml-stream
#[doc(alias = "l-yaml-stream")]
pub fn yaml_stream<'i, Input, Error>(input: &mut Input) -> winnow::Result<value::Stream<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("document::yaml_stream", move |input: &mut Input| {
        document_prefix.parse_next(input)?;
        let initial = opt(any_document).parse_next(input)?;
        let mut docs: Vec<Document> = initial.into_iter().collect();
        loop {
            let start = input.checkpoint();
            let (next, taken) = match dispatch! {
                peek(any);
                '.' => preceded(
                    (
                        repeat(1.., document_suffix::<_, Error>).map(|()|()),
                        repeat(0.., document_prefix).map(|()|()),
                    ),
                    opt(any_document)),
                chars::BOM => chars::BOM.value(None),
                '-' => explicit_document.map(Some),
                _ => spaces::line_comment.value(None),
            }
            .with_taken()
            .parse_next(input)
            {
                Err(err) if err.is_backtrack() => {
                    input.reset(&start);
                    return Ok(docs);
                }
                Err(err) => return Err(err),
                Ok(got) => got,
            };
            if taken.is_empty() {
                if input.eof_offset() == 0 {
                    return Ok(docs);
                }
                return Err(Error::assert(
                    input,
                    "stream element must consume at least one char except EOF",
                ));
            }
            if let Some(n) = next {
                docs.push(n);
            }
        }
    })
    .map(value::Stream)
    .parse_next(input)
}

/// Any document.
///
/// https://yaml.org/spec/1.2.2/#rule-l-any-document
#[doc(alias = "l-any-document")]
pub fn any_document<'i, Input, Error>(input: &mut Input) -> winnow::Result<Document<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::any_document",
        alt((directive_document, bare_document, explicit_document)),
    )
    .parse_next(input)
}

/// Directive document.
///
/// https://yaml.org/spec/1.2.2/#rule-l-directive-document
#[doc(alias = "l-directive-document")]
pub fn directive_document<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<Document<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::directive_document",
        // TODO: fixme implement this
        winnow::combinator::fail,
    )
    .parse_next(input)
}

/// Explicit document.
///
/// https://yaml.org/spec/1.2.2/#rule-l-explicit-document
#[doc(alias = "l-explicit-document")]
pub fn explicit_document<'i, Input, Error>(input: &mut Input) -> winnow::Result<Document<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::explicit_document",
        // TODO: fixme implement this
        winnow::combinator::fail,
    )
    .parse_next(input)
}

/// Bare document.
///
/// https://yaml.org/spec/1.2.2/#rule-l-bare-document
#[doc(alias = "l-bare-document")]
pub fn bare_document<'i, Input, Error>(input: &mut Input) -> winnow::Result<Document<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::bare_document",
        block::node::block_node(BlockIn, IndentLevel::initial()).map(Document::new),
    )
    .parse_next(input)
}

/// Document prefix.
///
/// https://yaml.org/spec/1.2.2/#rule-l-document-prefix
#[doc(alias = "l-document-prefix")]
pub fn document_prefix<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::document_prefix",
        (
            opt(chars::BOM),
            repeat(0.., spaces::line_comment).map(|()| ()),
        )
            .void(),
    )
    .parse_next(input)
}

/// Document suffix.
///
/// https://yaml.org/spec/1.2.2/#rule-l-document-suffix
#[doc(alias = "l-document-suffix")]
pub fn document_suffix<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::document_suffix",
        ("...", spaces::line_comments).void(),
    )
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;

    use crate::parse::testing;
    use crate::value::{Content, Node};

    #[test]
    fn simple_flow_seq() {
        let input = "['foo', 'bar', 'baz']";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Seq(
                    vec![
                        Node::unspecified(Content::Scalar(value::Scalar::SingleStr(
                            Cow::Borrowed("foo")
                        ))),
                        Node::unspecified(Content::Scalar(value::Scalar::SingleStr(
                            Cow::Borrowed("bar")
                        ))),
                        Node::unspecified(Content::Scalar(value::Scalar::SingleStr(
                            Cow::Borrowed("baz")
                        ))),
                    ]
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    #[test]
    fn simple_flow_map() {
        let input = "{'foo': 'bar'}\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Map(
                    value::Mapping(vec![value::MapEntry {
                        key: Node::unspecified(Content::Scalar(value::Scalar::SingleStr(
                            Cow::Borrowed("foo")
                        ))),
                        value: Node::unspecified(Content::Scalar(value::Scalar::SingleStr(
                            Cow::Borrowed("bar")
                        ))),
                    }])
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }
}
