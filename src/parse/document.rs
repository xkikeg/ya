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
/// Composed of the same two steps the lazy [`crate::Documents`] iterator drives -- [`stream_head`]
/// once, then [`stream_step`] until it reports [`StreamStep::End`] -- so that the eager and lazy
/// paths share one transliteration of the grammar rather than two.
///
/// https://yaml.org/spec/1.2.2/#rule-l-yaml-stream
#[doc(alias = "l-yaml-stream")]
pub fn yaml_stream<'i, Input, Error>(input: &mut Input) -> winnow::Result<value::Stream<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("document::yaml_stream", move |input: &mut Input| {
        let mut docs: Vec<Document> = stream_head::<_, Error>(input)?.into_iter().collect();
        loop {
            match stream_step::<_, Error>(input)? {
                StreamStep::Document(doc) => docs.push(doc),
                StreamStep::Skipped => {}
                StreamStep::End => return Ok(docs),
            }
        }
    })
    .map(value::Stream)
    .parse_next(input)
}

/// The outcome of one iteration of [`yaml_stream`]'s loop, i.e. one element of `l-yaml-stream`'s
/// trailing `( ... )*` group.
pub(crate) enum StreamStep<'i> {
    /// This iteration produced a document.
    Document(Document<'i>),
    /// This iteration consumed input (a BOM, a comment line, a `...` suffix) without producing a
    /// document.
    Skipped,
    /// There are no further stream elements; the input position is left untouched.
    End,
}

/// The head of [`l-yaml-stream`](yaml_stream): `l-document-prefix* l-any-document?`.
pub(crate) fn stream_head<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<Option<Document<'i>>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::stream_head",
        preceded(document_prefix, opt(any_document_reset)),
    )
    .parse_next(input)
}

/// One iteration of [`l-yaml-stream`](yaml_stream)'s trailing
/// `( l-document-suffix+ l-document-prefix* l-any-document? | l-document-prefix* l-explicit-document? )*`
/// group.
pub(crate) fn stream_step<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<StreamStep<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("document::stream_step", move |input: &mut Input| {
        let start = input.checkpoint();
        // Per `l-yaml-stream`, each iteration is *either* the `l-document-suffix+ ...
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
                opt(any_document_reset)),
            chars::BOM => chars::BOM.value(None),
            '-' => preceded(reset_document_state::<_, Error>, explicit_document).map(Some),
            _ => spaces::line_comment.value(None),
        }
        .with_taken()
        .parse_next(input)
        {
            Err(err) if err.is_backtrack() => {
                input.reset(&start);
                return Ok(StreamStep::End);
            }
            Err(err) => return Err(err),
            Ok(got) => got,
        };
        if taken.is_empty() {
            if input.eof_offset() == 0 {
                return Ok(StreamStep::End);
            }
            return Err(Error::assert(
                input,
                "stream element must consume at least one char except EOF",
            ));
        }
        Ok(match next {
            Some(doc) => StreamStep::Document(doc),
            None => StreamStep::Skipped,
        })
    })
    .parse_next(input)
}

/// A single document, with its surrounding `l-document-prefix*`.
///
/// **Not a spec production**: it's [`l-yaml-stream`](yaml_stream) restricted to exactly one
/// document, provided because parsing a single document is the common case (see
/// [`crate::parse_document`], which additionally rejects a stream that turns out to hold more).
/// Named after that intent rather than after a grammar rule, since there is no rule to name it
/// after.
pub fn yaml_document<'i, Input, Error>(input: &mut Input) -> winnow::Result<Document<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::yaml_document",
        preceded(document_prefix, any_document_reset),
    )
    .parse_next(input)
}

/// [`any_document`], preceded by the per-document state reset every document boundary needs (see
/// [`reset_document_state`]).
fn any_document_reset<'i, Input, Error>(input: &mut Input) -> winnow::Result<Document<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "document::any_document_reset",
        preceded(reset_document_state, any_document),
    )
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
/// **On `c-forbidden`** ([rule](https://yaml.org/spec/1.2.2/#rule-c-forbidden)): the spec bakes
/// this exclusion directly into this rule's own grammar -- formally, `l-bare-document` is
/// `s-l+block-node(-1, block-in)` restricted so that none of its lines may be `c-forbidden`
/// (a `---`/`...` marker at the start of a line). That's a *global* property of the whole node
/// this function produces, not something `bare_document` itself can check: it just delegates to
/// `block_node` and returns whatever `Document` comes back, and by then the marker line (if one
/// was swallowed) is already gone -- folded into a `Cow<str>` or consumed as part of some child
/// node, with no raw text left here to re-scan. Enforcing the exclusion after the fact would mean
/// re-lexing content that's already been parsed away.
///
/// So instead, each rule that *could* swallow a forbidden line guards against it individually, at
/// the point where the swallowing would happen. Most rules don't need a guard at all: a block
/// collection's entries are delimited by indentation (a `---`/`...` at column 0 is less indented
/// than any real nested content, so the collection ends there on its own), and a single-line
/// scalar or one with an explicit terminator (closing quote, `]`/`}`) can't run past a line it
/// hasn't reached yet. The only rules that keep consuming lines with *no* terminator other than
/// "this next line doesn't look like more content" are: line-folding across a multi-line scalar's
/// continuation lines, and a block scalar's `s-indent(n)` at the document root, where `n`
/// degenerates to 0 and so matches any line unconditionally. Those are exactly the sites that
/// carry their own `not(document::forbidden)` guard:
///
/// - `plain::plain` and `plain::space_non_space_plain_next_line` (a multi-line plain scalar's
///   first line and each continuation line, respectively),
/// - `double::non_break_double_multi_line` (a multi-line double-quoted scalar's fold, both the
///   escaped- and plain-line-break alternatives),
/// - `single::non_break_single_multi_line` (a multi-line single-quoted scalar's fold),
/// - `block::literal::literal_text` and `block::folded::folded_text` (a block scalar's content
///   lines, where this matters only at the document root -- everywhere else the scalar's own
///   positive indentation already discriminates a marker line the same way a block collection's
///   does).
///
/// Guarding every one of those sites is behaviorally equivalent to the spec's single exclusion
/// here at `l-bare-document`, since they're exhaustively the only paths that could ever reach a
/// forbidden line as if it were ordinary content -- but it avoids threading a "we're inside a
/// bare document, stop at markers" flag down through every intermediate rule (`block_node` and
/// everything it calls) just to reach the handful of leaf rules that actually need it.
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

    /// `yaml_document` parses one document and stops there, leaving the rest of a multi-document
    /// stream unconsumed for the caller to deal with (which is how `crate::parse_document` knows
    /// to reject it).
    #[test]
    fn single_document_stops_after_one_document() {
        let (rest, doc) = testing::parse(yaml_document, "'foo'\n---\n'bar'\n").unwrap();
        assert_eq!(
            doc,
            value::Document(Node::unspecified(Content::Scalar(
                value::Scalar::SingleStr(Cow::Borrowed("foo"))
            )))
        );
        assert_eq!(rest, "---\n'bar'\n");
    }

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

    /// Corpus case `ZH7C` ("Anchors in Mapping"): an anchor directly preceding an implicit block
    /// mapping key (`&a a: b`) anchors just that *key* node, not the whole mapping -- the
    /// enclosing `s-l+block-collection`'s own optional leading properties must backtrack when
    /// they're not followed by `s-l-comments` (i.e. when more of the same line follows), rather
    /// than hard-failing the whole collection.
    #[test]
    fn anchor_on_implicit_block_mapping_key_corpus_zh7c() {
        let input = "&a a: b\nc: &d d\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Map(
                    value::Mapping(vec![
                        value::MapEntry {
                            key: plain("a"),
                            value: plain("b"),
                        },
                        value::MapEntry {
                            key: plain("c"),
                            value: plain("d"),
                        },
                    ])
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }

    /// Corpus case `9KAX`'s last document ("Various combinations of tags and anchors"):
    /// `!!map\n&a8 !!str key8: value7`. The block collection's own tag (`!!map`) stands alone on
    /// its line, and the anchor on the *next* line (`&a8`) belongs to the nested mapping's first
    /// implicit key, not to the collection -- `properties::properties`'s own greedy trailing-
    /// anchor-after-tag lookup must not cross that line break and swallow it, the same class of
    /// bug `anchor_on_implicit_block_mapping_key_corpus_zh7c` covers one level up.
    #[test]
    fn tag_alone_on_its_line_does_not_swallow_next_lines_key_anchor_corpus_9kax() {
        let input = "!!map\n&a8 !!str key8: value7\n";
        let key8 = Node::new(
            Content::Scalar(value::Scalar::Plain(Cow::Borrowed("key8"))),
            value::Tag::Global(Cow::Borrowed("tag:yaml.org,2002:str")),
        );
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::new(
                    Content::Map(value::Mapping(vec![value::MapEntry {
                        key: key8,
                        value: plain("value7"),
                    }])),
                    value::Tag::Global(Cow::Borrowed("tag:yaml.org,2002:map")),
                ))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
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

    /// Corpus case `W4TN` (spec 9.5 "Directives Documents", full stream): a root-level (`n=-1`)
    /// block literal used to swallow the `...` document-end marker and everything after it as
    /// more of its own content, since `s-indent(0)` matches trivially and `literal_text` had no
    /// guard against a `---`/`...` marker line -- the same hazard already fixed for plain/quoted
    /// scalars, just unreachable for a root-level block scalar before the zero-indentation fix
    /// this test landed alongside. This is a genuine two-document stream: a directived document
    /// whose sole content is a block literal, followed by a second, empty directived document.
    #[test]
    fn stream_corpus_w4tn() {
        let input = "%YAML 1.2\n--- |\n%!PS-Adobe-2.0\n...\n%YAML 1.2\n---\n# Empty\n...\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![
                    value::Document(Node::unspecified(Content::Scalar(value::Scalar::Literal(
                        Cow::Borrowed("%!PS-Adobe-2.0\n")
                    )))),
                    value::Document(Node::unspecified(Content::Empty)),
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

    /// Corpus case `2EBW` ("Allowed characters in keys"): `?foo` (no space after `?`) is a plain
    /// scalar key, not an explicit-key marker -- `c-l-block-map-explicit-key`'s `?` is only the
    /// marker when *not* followed by a non-whitespace char (per the spec's own annotation on
    /// `c-mapping-key` at that rule), the same lookahead hazard Phase 0 already fixed for
    /// `c-l-block-seq-entry`'s `-`. Before that fix, `?foo: safe question mark` was misparsed as
    /// an explicit entry whose *key* was itself a nested one-entry mapping (`{foo: "safe question
    /// mark"}`), swallowing the whole line instead of treating `?foo` as one plain scalar.
    #[test]
    fn allowed_characters_in_keys_corpus_2ebw() {
        let input = "a!\"#$%&'()*+,-./09:;<=>?@AZ[\\]^_`az{|}~: safe\n?foo: safe question mark\n:foo: safe colon\n-foo: safe dash\nthis is#not: a comment\n";
        assert_eq!(
            (
                "",
                value::Stream(vec![value::Document(Node::unspecified(Content::Map(
                    value::Mapping(vec![
                        value::MapEntry {
                            key: plain("a!\"#$%&'()*+,-./09:;<=>?@AZ[\\]^_`az{|}~"),
                            value: plain("safe"),
                        },
                        value::MapEntry {
                            key: plain("?foo"),
                            value: plain("safe question mark"),
                        },
                        value::MapEntry {
                            key: plain(":foo"),
                            value: plain("safe colon"),
                        },
                        value::MapEntry {
                            key: plain("-foo"),
                            value: plain("safe dash"),
                        },
                        value::MapEntry {
                            key: plain("this is#not"),
                            value: plain("a comment"),
                        },
                    ])
                )))])
            ),
            testing::parse(yaml_stream, input).unwrap()
        );
    }
}
