//! Defines values result as a YAML parse.

use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub struct Stream<'i>(pub(crate) Vec<Document<'i>>);

#[derive(Debug, Clone, PartialEq)]
pub struct Document<'i>(pub(crate) Value<'i>);

impl<'i> Document<'i> {
    /// Creates a new instance.
    pub fn new(v: Value<'i>) -> Self {
        Self(v)
    }

    /// Takes the reference as [`Value`].
    pub fn as_value(&self) -> &Value<'i> {
        &self.0
    }

    /// Unwraps and returns the actual [`Value`].
    pub fn into_value(self) -> Value<'i> {
        self.0
    }
}

/// Represents a YAML object generically.
/// Note ideally I don't want to let serde impl rely on this.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'i> {
    Empty,
    Scalar(Scalar<'i>),
    Seq(Vec<Value<'i>>),
    Map(Mapping<'i>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar<'i> {
    Str(Cow<'i, str>),
    Signed(i64),
    Float(f64),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mapping<'i>(pub(crate) Vec<MapEntry<'i>>);

#[derive(Debug, Clone, PartialEq)]
pub struct MapEntry<'i> {
    pub key: Value<'i>,
    pub value: Value<'i>,
}

impl<'i> MapEntry<'i> {
    /// Constructs an instance out of a tuple.
    pub fn from_tuple((key, value): (Value<'i>, Value<'i>)) -> Self {
        Self { key, value }
    }
}
