use std::collections::HashSet;

use winnow::{
    combinator::{alt, dispatch, empty, eof, opt, peek, preceded, repeat, terminated, trace},
    error::{StrContext, StrContextValue},
    token::{any, take_while},
    Parser,
};

use crate::value::{self, Document};

use super::{
    block, chars,
    context::BlockIn,
    directive::{self, Directive},
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
        let initial = opt(preceded(reset_document_state::<_, Error>, any_document))
            .parse_next(input)?;
        let mut docs: Vec<Document> = initial.into_iter().collect();
        loop {
            let start = input.checkpoint();
            // Per `l-yaml-stream`, each loop iteration is *either* the `l-document-suffix+ ...
            // l-any-document?` branch (peeked '.': one or more `...` suffixes, then any kind of
            // document) *or* the `l-document-prefix* l-explicit-document?` branch (peeked '-':
            // only an *explicit* document, i.e. one starting with `---`) -- never a fresh
            // `l-directive-document` or bare document without a preceding `...` suffix. So a
            // leading '%' here (not right after a suffix) is deliberately left to the catch-all
            // arm, where it fails to parse as a comment and correctly ends the stream rather than
            // being accepted as a new directive document.
            let (next, taken) = match dispatch! {
                peek(any);
                '.' => preceded(
                    (
                        repeat(1.., document_suffix::<_, Error>).map(|()|()),
                        repeat(0.., document_prefix.take().verify(|s: &str| !s.is_empty()).void()).map(|()|()),
                    ),
                    opt(preceded(reset_document_state::<_, Error>, any_document))),
                chars::BOM => chars::BOM.value(None),
                '-' => preceded(reset_document_state::<_, Error>, explicit_document).map(Some),
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

/// Directive document: one or more directives (`%YAML`, `%TAG`, or reserved), followed by an
/// explicit document.
///
/// Hand-rolled rather than `repeat(1.., directive)`: registering a `%TAG` directive's handle ->
/// prefix mapping requires mutable access to `input`'s tag-handle map (see
/// [`super::input::WithTagHandles`]), and detecting a duplicate `%YAML`/handle within this
/// document requires tracking what's been seen so far -- neither fits a pure combinator chain.
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
    trace("document::directive_document", move |input: &mut Input| {
        let mut count = 0usize;
        let mut seen_yaml = false;
        let mut seen_handles: HashSet<&str> = HashSet::new();
        loop {
            let start = input.checkpoint();
            match directive::directive::<Input, Error>(input) {
                Ok(Directive::Yaml { .. }) => {
                    if seen_yaml {
                        return Err(Error::from_input(input).add_context(
                            input,
                            &start,
                            StrContext::Expected(StrContextValue::Description(
                                "at most one %YAML directive per document",
                            )),
                        ));
                    }
                    seen_yaml = true;
                    count += 1;
                }
                Ok(Directive::Tag { handle, prefix }) => {
                    if !seen_handles.insert(handle) {
                        return Err(Error::from_input(input).add_context(
                            input,
                            &start,
                            StrContext::Expected(StrContextValue::Description(
                                "each tag handle may be declared at most once per document",
                            )),
                        ));
                    }
                    input
                        .tag_handles_mut()
                        .put(std::borrow::Cow::Borrowed(handle), prefix);
                    count += 1;
                }
                Ok(Directive::Reserved) => {
                    count += 1;
                }
                Err(err) if err.is_backtrack() => {
                    input.reset(&start);
                    break;
                }
                Err(err) => return Err(err),
            }
        }
        if count == 0 {
            let checkpoint = input.checkpoint();
            return Err(Error::from_input(input).add_context(
                input,
                &checkpoint,
                StrContext::Expected(StrContextValue::Description("at least one directive")),
            ));
        }
        explicit_document(input)
    })
    .parse_next(input)
}

/// Explicit document: a `c-directives-end` marker (`---`), followed by either a bare document or
/// an empty node with trailing comments.
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
        preceded(
            directives_end,
            alt((
                bare_document,
                terminated(
                    empty.value(value::Node::unspecified(value::Content::Empty)),
                    spaces::line_comments,
                )
                .map(Document::new),
            )),
        ),
    )
    .parse_next(input)
}

/// Directives-end marker: literal `---`.
///
/// https://yaml.org/spec/1.2.2/#rule-c-directives-end
#[doc(alias = "c-directives-end")]
fn directives_end<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("document::directives_end", "---".void()).parse_next(input)
}

/// Resets per-document parse state (anchors and tag handles) so it doesn't leak across document
/// boundaries within the same stream: both are document-scoped
/// (https://yaml.org/spec/1.2.2/#3222-anchors-and-aliases for anchors; `%TAG` directives
/// similarly only apply to the document that declares them).
fn reset_document_state<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    input.anchor_store_mut().clear();
    input.tag_handles_mut().clear();
    Ok(())
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
            repeat(
                0..,
                spaces::line_comment
                    .take()
                    .verify(|s: &str| !s.is_empty())
                    .void(),
            )
            .map(|()| ()),
        )
            .void(),
    )
    .parse_next(input)
}

/// Content forbidden from appearing at the start of a line within a bare document: a
/// directives-end or document-end marker. Used to stop a multi-line plain scalar from swallowing
/// such a line as if it were ordinary content.
///
/// https://yaml.org/spec/1.2.2/#rule-c-forbidden
#[doc(alias = "c-forbidden")]
pub fn forbidden<'i, Input, Error>(input: &mut Input) -> winnow::Result<(), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::forbidden",
        (
            spaces::start_of_line,
            alt(("---", "...")),
            alt((
                chars::line_break.void(),
                take_while(1.., chars::is_white_space).void(),
                eof.void(),
            )),
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
    fn empty_input_parses_as_empty_stream() {
        let input = "";
        assert_eq!(
            ("", value::Stream(vec![])),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    #[test]
    fn dots_only_parses_as_empty_stream() {
        let input = "...";
        assert_eq!(
            ("", value::Stream(vec![])),
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

    #[test]
    fn simple_flow_seq_of_plain_scalars() {
        let input = "[one, two]";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Seq(
                    vec![
                        Node::unspecified(Content::Scalar(value::Scalar::Plain(Cow::Borrowed(
                            "one"
                        )))),
                        Node::unspecified(Content::Scalar(value::Scalar::Plain(Cow::Borrowed(
                            "two"
                        )))),
                    ]
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    #[test]
    fn anchor_and_alias_resolve_to_the_same_node() {
        let input = "[&a foo, *a]";
        let foo = Node::unspecified(Content::Scalar(value::Scalar::Plain(Cow::Borrowed("foo"))));
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Seq(
                    vec![foo.clone(), foo]
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    fn plain(s: &str) -> Node<'_> {
        Node::unspecified(Content::Scalar(value::Scalar::Plain(Cow::Borrowed(s))))
    }

    #[test]
    fn explicit_document_same_line_content() {
        assert_eq!(
            ("", value::Document(plain("foo"))),
            testing::parse(explicit_document, "--- foo\n").unwrap()
        );
    }

    #[test]
    fn explicit_document_empty_content() {
        assert_eq!(
            ("", value::Document(Node::unspecified(Content::Empty))),
            testing::parse(explicit_document, "---\n").unwrap()
        );
    }

    #[test]
    fn duplicate_yaml_directive_is_an_error() {
        let input = "%YAML 1.2\n%YAML 1.2\n---\n";
        testing::parse(directive_document, input).unwrap_err();
    }

    /// Spec example 6.15 "Invalid Repeated TAG Directive".
    #[test]
    fn duplicate_tag_directive_handle_spec_example_6_15() {
        let input =
            "%TAG ! tag:example.com,2000:app/\n%TAG ! tag:example.com,2000:different-app/\n---\n";
        testing::parse(directive_document, input).unwrap_err();
    }

    /// Spec example 9.1 "Document Prefix".
    #[test]
    fn document_prefix_spec_example_9_1() {
        let input = "%YAML 1.2\n--- text\n";
        assert_eq!(
            ("", value::Stream(vec![value::Document(plain("text"))])),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Spec example 9.2 "Document Markers": two documents, each a block sequence, with no
    /// suffix (`...`) between them -- exercises that a fresh `c-directives-end` can immediately
    /// follow the previous document's content.
    #[test]
    fn document_markers_spec_example_9_2() {
        let input =
            "---\n- Mark McGwire\n- Sammy Sosa\n- Ken Griffey\n\n---\n- Chicago Cubs\n- St Louis Cardinals\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![
                    value::Document(Node::unspecified(Content::Seq(vec![
                        plain("Mark McGwire"),
                        plain("Sammy Sosa"),
                        plain("Ken Griffey"),
                    ]))),
                    value::Document(Node::unspecified(Content::Seq(vec![
                        plain("Chicago Cubs"),
                        plain("St Louis Cardinals"),
                    ]))),
                ])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Spec example 9.4 "Explicit Documents": two documents separated by a `...` suffix, each a
    /// one-entry flow mapping.
    #[test]
    fn explicit_documents_spec_example_9_4() {
        let input = "---\n{foo: bar}\n...\n--- \n{baz: qux}\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![
                    value::Document(Node::unspecified(Content::Map(value::Mapping(vec![
                        value::MapEntry {
                            key: plain("foo"),
                            value: plain("bar"),
                        }
                    ])))),
                    value::Document(Node::unspecified(Content::Map(value::Mapping(vec![
                        value::MapEntry {
                            key: plain("baz"),
                            value: plain("qux"),
                        }
                    ])))),
                ])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Spec example 9.5 "Directives Documents": `%YAML` plus a `%TAG` redefining the primary
    /// handle, applied via a shorthand tag on the document's top-level block mapping.
    #[test]
    fn directives_document_spec_example_9_5() {
        let input = "%YAML 1.2\n%TAG ! tag:example.com,2000:\n---\n!Foo\nbar: baz\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::new(
                    Content::Map(value::Mapping(vec![value::MapEntry {
                        key: plain("bar"),
                        value: plain("baz"),
                    }])),
                    value::Tag::Global(Cow::Borrowed("tag:example.com,2000:Foo")),
                ))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Spec example 9.6 "Streams": two explicit documents, each `...`-terminated, with no
    /// directives -- the main multi-document stream regression case for this phase.
    #[test]
    fn stream_spec_example_9_6() {
        let input = "---\ntime: 20:03:20\nplayer: Sammy Sosa\naction: strike\n...\n---\ntime: 20:03:47\nplayer: Sammy Sosa\naction: grand slam\n...\n";
        fn game_event<'i>(time: &'i str, player: &'i str, action: &'i str) -> Node<'i> {
            Node::unspecified(Content::Map(value::Mapping(vec![
                value::MapEntry {
                    key: plain("time"),
                    value: plain(time),
                },
                value::MapEntry {
                    key: plain("player"),
                    value: plain(player),
                },
                value::MapEntry {
                    key: plain("action"),
                    value: plain(action),
                },
            ])))
        }
        assert_eq!(
            (
                "",
                value::Stream(vec![
                    value::Document(game_event("20:03:20", "Sammy Sosa", "strike")),
                    value::Document(game_event("20:03:47", "Sammy Sosa", "grand slam")),
                ])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Spec example 6.13 "Reserved Directives": an unrecognized `%FOO` directive (with a
    /// continuation comment line) is accepted and ignored.
    #[test]
    fn reserved_directive_spec_example_6_13() {
        let input =
            "%FOO  bar baz # Should be ignored\n               # with a warning.\n--- \"foo\"\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Scalar(
                    value::Scalar::DoubleStr(Cow::Borrowed("foo"))
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Spec example 6.16 "Tag Shorthands": a named handle (`!e!`) and a redefined primary handle
    /// (`!`), each used on a block-sequence entry.
    #[test]
    fn tag_shorthands_spec_example_6_16() {
        let input = "%TAG !e! tag:example.com,2000:\n%TAG ! tag:example.com,2000:app/\n---\n- !e!foo \"bar\"\n- !bar \"baz\"\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Seq(
                    vec![
                        Node::new(
                            Content::Scalar(value::Scalar::DoubleStr(Cow::Borrowed("bar"))),
                            value::Tag::Global(Cow::Borrowed("tag:example.com,2000:foo")),
                        ),
                        Node::new(
                            Content::Scalar(value::Scalar::DoubleStr(Cow::Borrowed("baz"))),
                            value::Tag::Global(Cow::Borrowed("tag:example.com,2000:app/bar")),
                        ),
                    ]
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Spec example 8.14 "Block Sequence": a mapping whose single value is a nested block
    /// sequence -- exercises `seq-space(n,c)` (BLOCK-OUT lets the nested sequence align with its
    /// own key's indentation).
    #[test]
    fn block_sequence_spec_example_8_14() {
        let input = "block sequence:\n  - one\n  - two : three\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Map(
                    value::Mapping(vec![value::MapEntry {
                        key: plain("block sequence"),
                        value: Node::unspecified(Content::Seq(vec![
                            plain("one"),
                            Node::unspecified(Content::Map(value::Mapping(vec![
                                value::MapEntry {
                                    key: plain("two"),
                                    value: plain("three"),
                                }
                            ]))),
                        ])),
                    }])
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }
}
