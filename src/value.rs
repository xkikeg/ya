//! Defines values result as a YAML parse.

use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub struct Stream<'i>(pub(crate) Vec<Document<'i>>);

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
    Unspecified,
    // TODO: should we have non-specific tags (?, !) or it must be resolved already?
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
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar<'i> {
    Str(Cow<'i, str>),
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mapping<'i>(pub(crate) Vec<MapEntry<'i>>);

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
