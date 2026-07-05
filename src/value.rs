//! Defines values result as a YAML parse.

use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub struct Stream<'i>(pub(crate) Vec<Document<'i>>);

impl<'i> Stream<'i> {
    /// Takes the reference to the documents in the stream.
    pub fn documents(&self) -> &[Document<'i>] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document<'i>(pub(crate) Node<'i>);

impl<'i> Document<'i> {
    /// Creates a new instance.
    pub fn new(v: Node<'i>) -> Self {
        Self(v)
    }

    /// Takes the reference as [`Node`].
    pub fn as_node(&self) -> &Node<'i> {
        &self.0
    }

    /// Unwraps and returns the actual [`Node`].
    pub fn into_node(self) -> Node<'i> {
        self.0
    }
}

/// Represents a YAML node, essentially value + properties.
#[derive(Debug, Clone, PartialEq)]
pub struct Node<'i> {
    pub value: Content<'i>,
    pub tag: Tag<'i>,
}

impl<'i> Node<'i> {
    /// Constructs a node with the tag.
    pub fn new(value: Content<'i>, tag: Tag<'i>) -> Self {
        Self { value, tag }
    }

    /// Constructs a node without properties.
    pub fn unspecified(value: Content<'i>) -> Self {
        Self {
            value,
            tag: Tag::Unspecified,
        }
    }
}

/// Tag of the YAML node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag<'i> {
    /// No tag property was given; eligible for core-schema resolution based on node kind and
    /// scalar style (e.g. a plain scalar `12` resolves to an int, but `"12"` stays a string).
    Unspecified,
    /// The tag property was explicitly set to the non-specific tag `!`
    /// ([`c-non-specific-tag`](https://yaml.org/spec/1.2.2/#rule-c-non-specific-tag)). Unlike
    /// `Unspecified`, this disables schema resolution: the node is forced to
    /// `tag:yaml.org,2002:{str,map,seq}` according to its kind, regardless of content.
    NonSpecific,
    Global(Cow<'i, str>),
    Standard(StandardTag),
}

/// Standard supported tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardTag {
    Map,
    Seq,
    Str,
    Null,
    Bool,
    Int,
    Float,
}

/// Represents a YAML content.
#[derive(Debug, Clone, PartialEq)]
pub enum Content<'i> {
    Empty,
    Scalar(Scalar<'i>),
    Seq(Vec<Node<'i>>),
    Map(Mapping<'i>),
}

/// Scalar of the YAML.
///
/// Deliberately purely textual: a scalar is its presentation style plus its content text, exactly
/// as in the spec's representation model. Native values (null/bool/int/float) are a
/// Construct-phase concern ([§3.1.2](https://yaml.org/spec/1.2.2/#312-construct)): tag resolution
/// (AGENT.md Phase 6) rewrites only [`Node::tag`] to [`Tag::Standard`], never the content, and
/// typed accessors / the serde layer interpret `(tag, text)` on demand. This keeps the source
/// lexeme recoverable (re-resolution under another schema, error messages, event-level
/// comparison) and defers numeric-range policy to the conversion the caller actually requests.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar<'i> {
    Plain(Cow<'i, str>),
    SingleStr(Cow<'i, str>),
    DoubleStr(Cow<'i, str>),
    /// A literal-style (`|`) block scalar. Always resolves to a string, like the quoted styles.
    Literal(Cow<'i, str>),
    /// A folded-style (`>`) block scalar. Always resolves to a string, like the quoted styles.
    Folded(Cow<'i, str>),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mapping<'i>(pub(crate) Vec<MapEntry<'i>>);

impl<'i> Mapping<'i> {
    /// Takes the reference to the entries in the mapping.
    pub fn entries(&self) -> &[MapEntry<'i>] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapEntry<'i> {
    pub key: Node<'i>,
    pub value: Node<'i>,
}

impl<'i> MapEntry<'i> {
    /// Constructs an instance out of a tuple.
    pub fn from_tuple((key, value): (Node<'i>, Node<'i>)) -> Self {
        Self { key, value }
    }
}
