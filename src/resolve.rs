//! Tag resolution against the [Core Schema](https://yaml.org/spec/1.2.2/#103-core-schema)
//! (AGENT.md Phase 6).
//!
//! [`resolve`] is a post-pass over an already-parsed [`Stream`]: it rewrites *tags only* (never
//! scalar content, see [`Scalar`]'s own doc comment for why), turning [`Tag::Unspecified`] and
//! most [`Tag::Global`] values into [`Tag::Standard`]. This keeps parsing itself schema-agnostic
//! (see the deferral comment on `parse::plain::plain`) and keeps the source lexeme recoverable
//! (re-resolution under another schema, quoting the original text in error messages, and
//! event-level comparison against the yaml-test-suite corpus, none of which would survive if
//! resolution rewrote scalar content in place).
//!
//! **Deviation from the plan's literal wording**: [`Tag::NonSpecific`] (the explicit bare `!`)
//! is *not* rewritten to `Tag::Standard` here, even though the Phase 6 design note originally
//! called for "`Tag::NonSpecific` -> str/map/seq by node kind". Checked against the yaml-test-suite
//! corpus directly (cases `52DL`/`8MK2`, "Explicit Non-Specific Tag"): `test.event` records an
//! explicitly-written `!` as the literal tag text `"!"`, not as its resolved kind
//! (`tag:yaml.org,2002:str`) -- i.e. the reference event stream operates at the *representation*
//! level (what was written), not the *construct* level (what it would ultimately become). Erasing
//! `NonSpecific` here would both contradict that ground truth and throw away real information (*why*
//! a node is being treated as str/map/seq -- explicit non-specific tag vs. core-schema classification
//! -- stops being recoverable). Construct-phase consumers that don't care about that distinction
//! (the [`crate::value::Node`] accessors, e.g. [`crate::value::Node::as_str`]) treat `NonSpecific`
//! as equivalent to its forced kind directly, without needing `resolve()` to have erased it first.

use std::borrow::Cow;

use crate::error::Excerpt;
use crate::value::{
    Content, Document, MapEntry, Mapping, Node, Scalar, Span, StandardTag, Stream, Tag,
};

/// Resolves every node's tag in `stream` against the Core Schema, consuming it and returning the
/// resolved stream. Scalar content is never rewritten (see the module docs); only [`Node::tag`]
/// changes, and only for [`Tag::Unspecified`] and canonical [`Tag::Global`] values (see the module
/// docs for why [`Tag::NonSpecific`] is deliberately left alone).
///
/// Fails only when an *explicit* standard tag (`!!int`, `!!map`, ...) doesn't match its node's
/// kind or, for scalars, the Core Schema pattern for that tag (e.g. `!!int foo`).
pub fn resolve(stream: Stream<'_>) -> Result<Stream<'_>, ResolveError> {
    let documents = stream
        .0
        .into_iter()
        .map(resolve_document)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Stream(documents))
}

/// Resolves a single document's tags, per [`resolve`] -- which is exactly this, mapped over the
/// stream's documents. Resolution never looks beyond the document it's given (a tag handle or an
/// anchor is document-scoped), so this is the unit the lazy [`crate::Documents`] iterator resolves
/// as it goes.
pub fn resolve_document(document: Document<'_>) -> Result<Document<'_>, ResolveError> {
    resolve_node(document.into_node()).map(Document::new)
}

/// A tag-resolution failure: an explicit standard tag was applied to content it can't legally
/// describe.
///
/// Carries the [`Span`] of the offending node, when the node came from the parser (nodes built by
/// hand have none). [`located`](Self::located) turns that span into a renderable source excerpt --
/// [`crate::parse_document`] and [`crate::parse_stream`] do it for you, since they have the input;
/// a caller driving [`resolve`] itself does it by hand, for the same reason.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveError {
    kind: ResolveErrorKind,
    span: Option<Span>,
    excerpt: Option<Excerpt>,
}

/// What went wrong, independent of where.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveErrorKind {
    /// An explicit standard tag (`!!map`/`!!seq`/`!!str`/...) requires a node of a specific kind
    /// (mapping/sequence/scalar) that the tagged node doesn't have.
    KindMismatch {
        tag: StandardTag,
        expected_kind: &'static str,
        actual_kind: &'static str,
    },
    /// An explicit `!!null`/`!!bool`/`!!int`/`!!float` tag was applied to scalar text that doesn't
    /// match that tag's Core Schema pattern.
    ContentMismatch { tag: StandardTag, text: String },
}

impl ResolveError {
    fn new(kind: ResolveErrorKind) -> Self {
        Self {
            kind,
            span: None,
            excerpt: None,
        }
    }

    /// What went wrong.
    pub fn kind(&self) -> &ResolveErrorKind {
        &self.kind
    }

    /// Where it went wrong, if the offending node carried a [`Span`].
    pub fn span(&self) -> Option<Span> {
        self.span
    }

    /// The source this error points at, once [`located`](Self::located) has been given the input.
    pub fn excerpt(&self) -> Option<&Excerpt> {
        self.excerpt.as_ref()
    }

    /// Attaches the source `source` was resolved from, so [`Display`](std::fmt::Display) can show
    /// the offending text rather than only describing it. A no-op if the error has no span.
    pub fn located(mut self, source: &str) -> Self {
        if let Some(span) = self.span {
            self.excerpt = Some(Excerpt::new(source, span));
        }
        self
    }

    /// Records `span` unless a nested node already recorded its own (a more precise one).
    fn at(mut self, span: Option<Span>) -> Self {
        if self.span.is_none() {
            self.span = span;
        }
        self
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::error::render(f, &self.kind.to_string(), self.excerpt.as_ref())
    }
}

impl std::fmt::Display for ResolveErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveErrorKind::KindMismatch {
                tag,
                expected_kind,
                actual_kind,
            } => write!(
                f,
                "explicit tag {tag:?} requires a {expected_kind} node, but found a {actual_kind}"
            ),
            ResolveErrorKind::ContentMismatch { tag, text } => write!(
                f,
                "explicit tag {tag:?} does not match scalar content {text:?}"
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

fn resolve_node(node: Node<'_>) -> Result<Node<'_>, ResolveError> {
    // Resolution rewrites the tag and rebuilds the node, so the span has to be carried across by
    // hand -- and stamped onto any error raised against this node, which is the whole point of
    // the parser recording it.
    let span = node.span();
    let content = resolve_content(node.value)?;
    let tag = resolve_tag(node.tag, &content).map_err(|err| err.at(span))?;
    let node = Node::new(content, tag);
    Ok(match span {
        Some(span) => node.with_span(span),
        None => node,
    })
}

fn resolve_content(content: Content<'_>) -> Result<Content<'_>, ResolveError> {
    match content {
        Content::Empty => Ok(Content::Empty),
        Content::Scalar(s) => Ok(Content::Scalar(s)),
        Content::Seq(items) => Ok(Content::Seq(
            items
                .into_iter()
                .map(resolve_node)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Content::Map(mapping) => {
            let entries = mapping
                .0
                .into_iter()
                .map(|entry| {
                    Ok(MapEntry {
                        key: resolve_node(entry.key)?,
                        value: resolve_node(entry.value)?,
                    })
                })
                .collect::<Result<Vec<_>, ResolveError>>()?;
            Ok(Content::Map(Mapping(entries)))
        }
    }
}

fn resolve_tag<'i>(tag: Tag<'i>, content: &Content<'i>) -> Result<Tag<'i>, ResolveError> {
    match tag {
        Tag::Standard(t) => Ok(Tag::Standard(t)),
        Tag::NonSpecific => Ok(Tag::NonSpecific),
        Tag::Unspecified => Ok(Tag::Standard(unspecified_tag(content))),
        Tag::Global(uri) => {
            let decoded = decode_tag_uri(&uri);
            match explicit_standard_tag(&decoded) {
                Some(tag) => {
                    validate_explicit_standard(tag, content)?;
                    Ok(Tag::Standard(tag))
                }
                None => Ok(Tag::Global(Cow::Owned(decoded))),
            }
        }
    }
}

/// `Tag::Unspecified`'s resolution: collections resolve by kind; an empty node resolves to null
/// (matching the Core Schema table's `/* Empty */` -> `tag:yaml.org,2002:null` row); a plain
/// scalar is classified against the Core Schema patterns, falling back to `str`; any other scalar
/// style (single/double-quoted, literal, folded) is always `str`, since only plain scalars are
/// eligible for Core Schema classification.
fn unspecified_tag(content: &Content<'_>) -> StandardTag {
    match content {
        Content::Seq(_) => StandardTag::Seq,
        Content::Map(_) => StandardTag::Map,
        Content::Empty => StandardTag::Null,
        Content::Scalar(Scalar::Plain(text)) => {
            classify_core_schema(text).unwrap_or(StandardTag::Str)
        }
        Content::Scalar(_) => StandardTag::Str,
    }
}

/// Maps a (already percent-decoded) tag URI to a [`StandardTag`] if it's one of the seven
/// canonical `tag:yaml.org,2002:*` URIs; any other URI (a custom/local tag) isn't a standard tag.
fn explicit_standard_tag(uri: &str) -> Option<StandardTag> {
    Some(match uri {
        "tag:yaml.org,2002:map" => StandardTag::Map,
        "tag:yaml.org,2002:seq" => StandardTag::Seq,
        "tag:yaml.org,2002:str" => StandardTag::Str,
        "tag:yaml.org,2002:null" => StandardTag::Null,
        "tag:yaml.org,2002:bool" => StandardTag::Bool,
        "tag:yaml.org,2002:int" => StandardTag::Int,
        "tag:yaml.org,2002:float" => StandardTag::Float,
        _ => return None,
    })
}

/// Validates that an *explicit* standard tag is well-formed for the content it was applied to:
/// map/seq/str require the matching node kind, and null/bool/int/float additionally require the
/// scalar text to match that tag's Core Schema pattern.
fn validate_explicit_standard(tag: StandardTag, content: &Content<'_>) -> Result<(), ResolveError> {
    match tag {
        StandardTag::Map => require_kind(tag, "mapping", content),
        StandardTag::Seq => require_kind(tag, "sequence", content),
        StandardTag::Str => require_kind(tag, "scalar", content),
        StandardTag::Null | StandardTag::Bool | StandardTag::Int | StandardTag::Float => {
            require_kind(tag, "scalar", content)?;
            let text = scalar_text(content);
            if classify_core_schema(text) == Some(tag) {
                Ok(())
            } else {
                Err(ResolveError::new(ResolveErrorKind::ContentMismatch {
                    tag,
                    text: text.to_string(),
                }))
            }
        }
    }
}

fn require_kind(
    tag: StandardTag,
    expected_kind: &'static str,
    content: &Content<'_>,
) -> Result<(), ResolveError> {
    let actual_kind = content_kind(content);
    if actual_kind == expected_kind {
        Ok(())
    } else {
        Err(ResolveError::new(ResolveErrorKind::KindMismatch {
            tag,
            expected_kind,
            actual_kind,
        }))
    }
}

fn content_kind(content: &Content<'_>) -> &'static str {
    match content {
        Content::Empty | Content::Scalar(_) => "scalar",
        Content::Seq(_) => "sequence",
        Content::Map(_) => "mapping",
    }
}

/// A node's scalar text, for content already known to be scalar-kind (i.e. after
/// [`require_kind`] has already confirmed `content_kind(content) == "scalar"`).
fn scalar_text<'a>(content: &'a Content<'_>) -> &'a str {
    match content {
        Content::Empty => "",
        Content::Scalar(
            Scalar::Plain(t)
            | Scalar::SingleStr(t)
            | Scalar::DoubleStr(t)
            | Scalar::Literal(t)
            | Scalar::Folded(t),
        ) => t,
        Content::Seq(_) | Content::Map(_) => {
            unreachable!("scalar_text called on non-scalar content; caller must require_kind first")
        }
    }
}

// --- Core Schema classifiers (https://yaml.org/spec/1.2.2/#1032-tag-resolution) -----------------

/// Classifies `text` (a plain scalar's content, or an explicitly-tagged scalar's content) against
/// the Core Schema's null/bool/int/float patterns, in the table's own listed order (each pattern
/// is mutually exclusive from the others actually reachable here, but int's decimal form is a
/// textual prefix of float's, so order still matters). Returns `None` if nothing matches (falls
/// back to `str`).
fn classify_core_schema(text: &str) -> Option<StandardTag> {
    if is_null(text) {
        Some(StandardTag::Null)
    } else if is_bool(text) {
        Some(StandardTag::Bool)
    } else if is_int(text) {
        Some(StandardTag::Int)
    } else if is_float(text) {
        Some(StandardTag::Float)
    } else {
        None
    }
}

/// `null | Null | NULL | ~ | /* Empty */`.
fn is_null(s: &str) -> bool {
    matches!(s, "null" | "Null" | "NULL" | "~" | "")
}

/// `true | True | TRUE | false | False | FALSE`.
fn is_bool(s: &str) -> bool {
    matches!(s, "true" | "True" | "TRUE" | "false" | "False" | "FALSE")
}

/// `[-+]? [0-9]+ | 0o [0-7]+ | 0x [0-9a-fA-F]+`.
fn is_int(s: &str) -> bool {
    is_dec_int(s)
        || is_radix_int(s, "0o", |c| ('0'..='7').contains(&c))
        || is_radix_int(s, "0x", |c| c.is_ascii_hexdigit())
}

fn is_dec_int(s: &str) -> bool {
    is_digits(strip_sign(s), |c| c.is_ascii_digit())
}

/// Strips a leading `-`/`+` sign, if present. A tiny helper rather than
/// `s.strip_prefix(['-', '+'])`, since array/slice char patterns for `strip_prefix`/`find` need
/// Rust 1.71, one past this crate's declared `rust-version = "1.70.0"`.
fn strip_sign(s: &str) -> &str {
    s.strip_prefix('-')
        .or_else(|| s.strip_prefix('+'))
        .unwrap_or(s)
}

fn is_radix_int(s: &str, prefix: &str, digit: fn(char) -> bool) -> bool {
    s.strip_prefix(prefix)
        .is_some_and(|rest| is_digits(rest, digit))
}

fn is_digits(s: &str, digit: impl Fn(char) -> bool) -> bool {
    !s.is_empty() && s.chars().all(digit)
}

/// `[-+]? ( \.[0-9]+ | [0-9]+ ( \.[0-9]* )? ) ( [eE] [-+]? [0-9]+ )?`
/// `| [-+]? ( \.inf | \.Inf | \.INF ) | \.nan | \.NaN | \.NAN`.
fn is_float(s: &str) -> bool {
    is_signed(s, |rest| matches!(rest, ".inf" | ".Inf" | ".INF"))
        || matches!(s, ".nan" | ".NaN" | ".NAN")
        || is_regular_float(s)
}

fn is_signed(s: &str, pred: impl Fn(&str) -> bool) -> bool {
    pred(strip_sign(s))
}

fn is_regular_float(s: &str) -> bool {
    let rest = strip_sign(s);
    let (mantissa, exponent) = match rest.find('e').or_else(|| rest.find('E')) {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    is_float_mantissa(mantissa) && exponent.is_none_or(is_dec_int)
}

fn is_float_mantissa(s: &str) -> bool {
    if let Some(frac) = s.strip_prefix('.') {
        return !frac.is_empty() && is_digits(frac, |c| c.is_ascii_digit());
    }
    let (int_part, frac) = match s.find('.') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    is_digits(int_part, |c| c.is_ascii_digit())
        && frac.is_none_or(|f| f.chars().all(|c| c.is_ascii_digit()))
}

/// Percent-decodes `%XX` escapes in a tag URI. Shorthand tags keep escapes raw at parse time (see
/// `parse::properties::tag_chars`'s doc comment: "decoding is a resolution concern"), so this is
/// where that deferred decoding actually happens. A multi-byte UTF-8 character can be spread
/// across consecutive `%XX` escapes, so bytes are collected first and the whole buffer is
/// validated as UTF-8 once at the end, same as ordinary URI percent-decoding.
fn decode_tag_uri(uri: &str) -> String {
    if !uri.contains('%') {
        return uri.to_string();
    }
    let bytes = uri.as_bytes();
    let mut out = Vec::with_capacity(uri.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&uri[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        let ch_len = uri[i..].chars().next().map_or(1, char::len_utf8);
        out.extend_from_slice(&bytes[i..i + ch_len]);
        i += ch_len;
    }
    // Malformed escape (not valid UTF-8 once decoded): keep the raw text rather than losing data.
    String::from_utf8(out).unwrap_or_else(|_| uri.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::value::{Document, MapEntry, Mapping};

    fn plain(text: &str) -> Node<'_> {
        Node::unspecified(Content::Scalar(Scalar::Plain(Cow::Borrowed(text))))
    }

    fn resolved(text: &str) -> Tag<'static> {
        Tag::Standard(classify_core_schema(text).unwrap_or(StandardTag::Str))
    }

    /// Spec's Core Schema resolution table (https://yaml.org/spec/1.2.2/#1032-tag-resolution),
    /// checked against its own "Core Tag Resolution" worked example (same section): every one of
    /// its entries, transcribed verbatim.
    #[test]
    fn core_schema_table() {
        // null
        for s in ["null", "Null", "NULL", "~", ""] {
            assert_eq!(Some(StandardTag::Null), classify_core_schema(s), "{s:?}");
        }
        // bool
        for s in ["true", "True", "TRUE", "false", "False", "FALSE"] {
            assert_eq!(Some(StandardTag::Bool), classify_core_schema(s), "{s:?}");
        }
        // int
        for s in ["0", "0o7", "0x3A", "-19", "+19", "123"] {
            assert_eq!(Some(StandardTag::Int), classify_core_schema(s), "{s:?}");
        }
        // float
        for s in [
            "0.", "-0.0", ".5", "+12e03", "-2E+05", ".inf", "-.Inf", "+.INF", ".NAN",
        ] {
            assert_eq!(Some(StandardTag::Float), classify_core_schema(s), "{s:?}");
        }
        // default: str
        for s in ["yes", "no", "on", "off", "1.2.3", "foo"] {
            assert_eq!(None, classify_core_schema(s), "{s:?}");
        }
    }

    /// The Core Schema's own worked example ("Core Tag Resolution", §10.3.2), transcribed
    /// verbatim (as a flow mapping, to keep the test focused on tag resolution rather than block
    /// mapping parsing, which is already covered elsewhere).
    #[test]
    fn core_tag_resolution_example() {
        let input = concat!(
            "{",
            "'A null': null, ",
            "'Also a null': , ",
            "'Not a null': \"\", ",
            "Booleans: [true, True, false, FALSE], ",
            "Integers: [0, 0o7, 0x3A, -19], ",
            "Floats: [0., -0.0, .5, +12e03, -2E+05], ",
            "Also floats: [.inf, -.Inf, +.INF, .NAN]",
            "}",
        );
        let (rest, stream) =
            crate::parse::testing::parse(crate::parse::yaml_stream, input).unwrap();
        assert_eq!("", rest);
        let resolved_stream = resolve(stream).unwrap();
        let doc = &resolved_stream.documents()[0];
        let Content::Map(mapping) = &doc.as_node().value else {
            panic!("expected a mapping, got {:?}", doc.as_node().value);
        };
        let entries = mapping.entries();

        assert_eq!(Tag::Standard(StandardTag::Null), entries[0].value.tag);
        assert_eq!(Tag::Standard(StandardTag::Null), entries[1].value.tag);
        assert_eq!(Tag::Standard(StandardTag::Str), entries[2].value.tag);

        let Content::Seq(booleans) = &entries[3].value.value else {
            panic!("expected a sequence");
        };
        for item in booleans {
            assert_eq!(Tag::Standard(StandardTag::Bool), item.tag);
        }

        let Content::Seq(integers) = &entries[4].value.value else {
            panic!("expected a sequence");
        };
        for item in integers {
            assert_eq!(Tag::Standard(StandardTag::Int), item.tag);
        }

        let Content::Seq(floats) = &entries[5].value.value else {
            panic!("expected a sequence");
        };
        for item in floats {
            assert_eq!(Tag::Standard(StandardTag::Float), item.tag);
        }

        let Content::Seq(also_floats) = &entries[6].value.value else {
            panic!("expected a sequence");
        };
        for item in also_floats {
            assert_eq!(Tag::Standard(StandardTag::Float), item.tag);
        }
    }

    #[test]
    fn unspecified_collections_resolve_by_kind() {
        assert_eq!(StandardTag::Seq, unspecified_tag(&Content::Seq(vec![])));
        assert_eq!(
            StandardTag::Map,
            unspecified_tag(&Content::Map(Mapping::default()))
        );
        assert_eq!(StandardTag::Null, unspecified_tag(&Content::Empty));
    }

    #[test]
    fn unspecified_non_plain_scalar_is_always_str() {
        assert_eq!(
            StandardTag::Str,
            unspecified_tag(&Content::Scalar(Scalar::SingleStr(Cow::Borrowed("42"))))
        );
        assert_eq!(
            StandardTag::Str,
            unspecified_tag(&Content::Scalar(Scalar::DoubleStr(Cow::Borrowed("true"))))
        );
    }

    #[test]
    fn non_specific_tag_is_left_unresolved() {
        // Corpus cases 52DL/8MK2 ("Explicit Non-Specific Tag"): the reference event stream
        // records an explicit bare `!` as the literal tag "!", not its resolved kind -- see the
        // module docs for the full rationale.
        let node = Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed("a"))),
            Tag::NonSpecific,
        );
        let resolved = resolve_node(node).unwrap();
        assert_eq!(Tag::NonSpecific, resolved.tag);
    }

    #[test]
    fn explicit_standard_tag_validates_content() {
        let ok = Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed("42"))),
            Tag::Global(Cow::Borrowed("tag:yaml.org,2002:int")),
        );
        assert_eq!(
            Tag::Standard(StandardTag::Int),
            resolve_node(ok).unwrap().tag
        );

        let bad = Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed("foo"))),
            Tag::Global(Cow::Borrowed("tag:yaml.org,2002:int")),
        );
        assert_eq!(
            &ResolveErrorKind::ContentMismatch {
                tag: StandardTag::Int,
                text: "foo".to_string()
            },
            resolve_node(bad).unwrap_err().kind()
        );
    }

    #[test]
    fn explicit_standard_tag_validates_kind() {
        let bad = Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed("foo"))),
            Tag::Global(Cow::Borrowed("tag:yaml.org,2002:map")),
        );
        assert_eq!(
            &ResolveErrorKind::KindMismatch {
                tag: StandardTag::Map,
                expected_kind: "mapping",
                actual_kind: "scalar",
            },
            resolve_node(bad).unwrap_err().kind()
        );
    }

    #[test]
    fn custom_global_tag_stays_global() {
        let node = Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed("foo"))),
            Tag::Global(Cow::Borrowed("tag:example.com,2000:app/foo")),
        );
        assert_eq!(
            Tag::Global(Cow::Borrowed("tag:example.com,2000:app/foo")),
            resolve_node(node).unwrap().tag
        );
    }

    /// Corpus case 6CK3 ("Spec Example 6.26. Tag Shorthands"): a `%XX`-escaped shorthand suffix
    /// (`tag%21`) must be percent-decoded (to `tag!`) as part of resolution.
    #[test]
    fn global_tag_percent_escape_is_decoded() {
        let node = Node::new(
            Content::Scalar(Scalar::DoubleStr(Cow::Borrowed("baz"))),
            Tag::Global(Cow::Borrowed("tag:example.com,2000:app/tag%21")),
        );
        assert_eq!(
            Tag::Global(Cow::Owned("tag:example.com,2000:app/tag!".to_string())),
            resolve_node(node).unwrap().tag
        );
    }

    /// The point of Phase 10: a mistagged node names the text it was written as, not just what's
    /// wrong with it.
    #[test]
    fn resolve_error_carries_the_offending_node_span() {
        let input = "a: 1\nb: !!int nope\n";
        let err = crate::parse_document(input).unwrap_err();
        let crate::Error::Resolve(err) = err else {
            panic!("expected a resolution error, got {err:?}");
        };
        assert_eq!(
            &input[err.span().expect("a parsed node has a span").range()],
            "!!int nope"
        );
        assert_eq!(err.excerpt().unwrap().line(), 2);
    }

    /// One of the three exact-output tests -- see `crate::error`'s own for the rationale.
    #[test]
    fn resolve_error_renders_the_offending_source() {
        let err = crate::parse_document("a: 1\nb: !!int nope\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            "\
error: explicit tag Int does not match scalar content \"nope\"
  |
2 | b: !!int nope
  |    ^^^^^^^^^^"
        );
    }

    /// A node built by hand has no span, so there is nothing to point at -- the message alone is
    /// still a complete error.
    #[test]
    fn resolve_error_without_a_span_renders_its_message_alone() {
        let node = Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed("foo"))),
            Tag::Global(Cow::Borrowed("tag:yaml.org,2002:int")),
        );
        let err = resolve_node(node).unwrap_err();
        assert_eq!(err.span(), None);
        assert_eq!(
            err.located("irrelevant, there is no span to locate")
                .to_string(),
            "explicit tag Int does not match scalar content \"foo\""
        );
    }

    #[test]
    fn resolve_recurses_into_collections() {
        let stream = Stream(vec![Document::new(Node::unspecified(Content::Map(
            Mapping(vec![MapEntry {
                key: plain("a"),
                value: Node::unspecified(Content::Seq(vec![plain("1"), plain("true")])),
            }]),
        )))]);
        let resolved_stream = resolve(stream).unwrap();
        let doc = &resolved_stream.documents()[0];
        let Content::Map(mapping) = &doc.as_node().value else {
            panic!("expected a mapping");
        };
        assert_eq!(resolved("a"), mapping.entries()[0].key.tag);
        let Content::Seq(items) = &mapping.entries()[0].value.value else {
            panic!("expected a sequence");
        };
        assert_eq!(Tag::Standard(StandardTag::Int), items[0].tag);
        assert_eq!(Tag::Standard(StandardTag::Bool), items[1].tag);
    }
}
