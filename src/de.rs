//! Optional [`serde::Deserialize`] support (Cargo feature `serde`), completing the Construct phase
//! this crate's design has always deferred to it: [`crate::resolve`] resolves tags but leaves
//! scalar content as written, and this layer -- like the [`Node`] accessors it sits above -- parses
//! that retained lexeme against the caller's requested type at conversion time.
//!
//! There are two deserializer types, mirroring `serde_json`'s split:
//!
//! * [`Deserializer`] is built from the format's own input ([`Deserializer::from_str`]) and is the
//!   type serde's [conventions](https://serde.rs/conventions.html) call for. It deserializes a
//!   single-document stream directly, and yields one value per document via
//!   [`into_iter`](Deserializer::into_iter) for a multi-document one.
//! * [`NodeDeserializer`] works one [`Node`] at a time, for callers who already hold a parsed node
//!   (also reachable as [`Node::into_deserializer`](serde::de::IntoDeserializer::into_deserializer)).
//!   It consumes its node, so borrowed scalar text (`Cow::Borrowed`) reaches the visitor zero-copy
//!   via `visit_borrowed_str`, exactly like the parser's own `Cow`-borrowing discipline.
//!
//! [`from_str`] and [`from_bytes`] are the one-shot entry points, mirroring
//! `serde_json`/`serde_yaml`'s own.
//!
//! Only `Deserialize` is implemented, not `Serialize`: the tag-only resolution model
//! makes a `Node -> T` conversion a natural, lossy-by-design projection (an `!!int`'s
//! text is parsed against whatever integer type the caller asks for), whereas `T -> Node` would
//! need to invent presentation-layer decisions (style, quoting) this crate doesn't model at all.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{
    self, Deserialize, DeserializeSeed, EnumAccess, Error as _, IntoDeserializer, MapAccess,
    SeqAccess, VariantAccess, Visitor,
};

use crate::documents::{single_document, Documents};
use crate::error::Excerpt;
use crate::value::{parse_core_float, parse_core_int, parse_core_uint};
use crate::value::{Content, MapEntry, Mapping, Node, Scalar, Span, StandardTag, Tag};
use crate::Error;

/// Deserializes `input` as YAML into `T`.
///
/// An empty stream (`input` containing zero documents, e.g. `""` or a comment-only file)
/// deserializes as an implicit null node, matching the Core Schema's own treatment of an empty
/// document ([§10.3.2](https://yaml.org/spec/1.2.2/#1032-tag-resolution)). A stream with more than
/// one document is rejected -- use [`Deserializer::into_iter`] for those.
///
/// ```
/// #[derive(serde::Deserialize, Debug, PartialEq)]
/// struct Point {
///     x: i64,
///     y: i64,
/// }
///
/// let point: Point = ya::de::from_str("x: 1\ny: 2\n").unwrap();
/// assert_eq!(point, Point { x: 1, y: 2 });
/// ```
pub fn from_str<'de, T>(input: &'de str) -> crate::Result<T>
where
    T: Deserialize<'de>,
{
    T::deserialize(Deserializer::from_str(input))
}

/// Deserializes `input` as UTF-8 encoded YAML into `T`.
///
/// Equivalent to [`from_str`] after a UTF-8 check; invalid UTF-8 becomes [`Error::Utf8`]. There is
/// deliberately no `from_reader`: it would need to own a buffer, which defeats this crate's
/// `Cow::Borrowed` zero-copy discipline and would force `T: DeserializeOwned`. Read to a `String`
/// and call [`from_str`] instead.
///
/// ```
/// let bytes: &[u8] = b"- 1\n- 2\n";
/// assert_eq!(ya::de::from_bytes::<Vec<i64>>(bytes).unwrap(), vec![1, 2]);
/// ```
pub fn from_bytes<'de, T>(input: &'de [u8]) -> crate::Result<T>
where
    T: Deserialize<'de>,
{
    from_str(std::str::from_utf8(input).map_err(Error::Utf8)?)
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Deserialize {
            message: msg.to_string(),
            excerpt: None,
        }
    }
}

/// Where the node currently being deserialized came from, so an error raised against it can point
/// at its source text.
///
/// serde's [`de::Error::custom`] is a free function with no access to any of this -- it's how every
/// derived `Deserialize` reports a missing field or a type mismatch -- so the location is attached
/// afterwards, by the deserializer that was running when the error came back.
#[derive(Clone, Copy, Default)]
struct Origin<'de> {
    span: Option<Span>,
    source: Option<&'de str>,
}

impl<'de> Origin<'de> {
    /// Points `err` at this node, unless it already points at one.
    ///
    /// Only-if-missing, so the innermost deserializer still running when the error surfaced wins:
    /// a bad `x` in `{x: nope}` points at `nope`, not at the whole mapping that contains it.
    fn locate(self, err: Error) -> Error {
        match (err, self.span, self.source) {
            (
                Error::Deserialize {
                    message,
                    excerpt: None,
                },
                Some(span),
                Some(source),
            ) => Error::Deserialize {
                message,
                excerpt: Some(Excerpt::new(source, span)),
            },
            (err, _, _) => err,
        }
    }
}

/// A [`serde::Deserializer`] over a whole YAML input.
///
/// Deserializing this directly expects a single-document stream (an empty one counts as null);
/// [`into_iter`](Self::into_iter) handles a `---`-separated multi-document stream instead.
///
/// ```
/// #[derive(serde::Deserialize, Debug, PartialEq)]
/// struct Point {
///     x: i64,
///     y: i64,
/// }
///
/// let points: Vec<Point> = ya::Deserializer::from_str("x: 1\ny: 2\n---\nx: 3\ny: 4\n")
///     .into_iter()
///     .collect::<Result<_, _>>()
///     .unwrap();
/// assert_eq!(points, vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
/// ```
pub struct Deserializer<'de> {
    documents: Documents<'de>,
}

impl<'de> Deserializer<'de> {
    /// Creates a deserializer over `input`.
    ///
    /// Infallible, because nothing is parsed yet: [`crate::parse_stream`] parses each document only
    /// when it's asked for, so a syntax error surfaces from the deserialization itself. This is
    /// also `serde_json::Deserializer::from_str`'s signature.
    // Not `std::str::FromStr`: that trait can't borrow from its input, which is the entire point
    // of this type. Named `from_str` to match `serde_json::Deserializer::from_str`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &'de str) -> Self {
        Self {
            documents: crate::parse_stream(input),
        }
    }

    /// Like [`from_str`](Self::from_str), for UTF-8 encoded input. Fallible only because of the
    /// UTF-8 check.
    pub fn from_bytes(input: &'de [u8]) -> crate::Result<Self> {
        Ok(Self::from_str(
            std::str::from_utf8(input).map_err(Error::Utf8)?,
        ))
    }

    /// Turns this into an iterator yielding one `T` per document in the stream.
    ///
    /// Lazy, like `serde_json::StreamDeserializer`: each document is parsed, resolved and
    /// deserialized when the iterator reaches it, so a stream costs only its largest single
    /// document in peak memory. (It isn't lazy over *input* the way serde_json's is -- see
    /// [`crate::parse_stream`] for that distinction.)
    // Not `IntoIterator`: the element type is chosen by the caller (`into_iter::<T>()`), which a
    // trait impl can't express. Same signature as `serde_json::Deserializer::into_iter`.
    #[allow(clippy::should_implement_trait)]
    pub fn into_iter<T>(self) -> StreamDeserializer<'de, T>
    where
        T: Deserialize<'de>,
    {
        StreamDeserializer {
            documents: self.documents,
            marker: PhantomData,
        }
    }

    /// Reduces the stream to the single node to deserialize, per [`from_str`]'s documented rules.
    fn into_single(self) -> crate::Result<NodeDeserializer<'de>> {
        let source = self.documents.source();
        Ok(NodeDeserializer::with_source(
            single_document(self.documents)?.into_node(),
            source,
        ))
    }
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_any(self.into_single()?, visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_option(self.into_single()?, visitor)
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_enum(self.into_single()?, name, variants, visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

/// An iterator over the documents of a [`Deserializer`], deserializing each into a `T` as it's
/// reached.
///
/// Created by [`Deserializer::into_iter`]. Deliberately not an [`ExactSizeIterator`]: the documents
/// after the current one haven't been parsed yet, so their count isn't known.
#[must_use = "`StreamDeserializer` is an iterator, and parses nothing unless consumed"]
pub struct StreamDeserializer<'de, T> {
    documents: Documents<'de>,
    marker: PhantomData<T>,
}

impl<'de, T> Iterator for StreamDeserializer<'de, T>
where
    T: Deserialize<'de>,
{
    type Item = crate::Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let source = self.documents.source();
        self.documents
            .next()
            .map(|doc| T::deserialize(NodeDeserializer::with_source(doc?.into_node(), source)))
    }
}

impl<'de, T> std::iter::FusedIterator for StreamDeserializer<'de, T> where T: Deserialize<'de> {}

/// A [`serde::Deserializer`] over an owned [`Node`]. See the module docs for why it consumes
/// rather than borrows its node.
pub struct NodeDeserializer<'de> {
    node: Node<'de>,
    /// The input the node was parsed from, when known. Only needed to render an error against the
    /// node's [`Span`]; deserializing itself never looks at it.
    source: Option<&'de str>,
}

impl<'de> NodeDeserializer<'de> {
    /// Creates a new instance wrapping `node`.
    ///
    /// Errors from it carry no source excerpt: a `Node` knows the byte range it was parsed from,
    /// but not the text. Use [`with_source`](Self::with_source) -- or go through
    /// [`Deserializer`]/[`from_str`], which do -- for located errors.
    pub fn new(node: Node<'de>) -> Self {
        Self { node, source: None }
    }

    /// Like [`new`](Self::new), but able to render errors against the input `node` was parsed
    /// from.
    pub fn with_source(node: Node<'de>, source: &'de str) -> Self {
        Self {
            node,
            source: Some(source),
        }
    }

    fn origin(&self) -> Origin<'de> {
        Origin {
            span: self.node.span(),
            source: self.source,
        }
    }
}

/// Lets a [`Node`] be used wherever serde asks for a deserializer, the way `serde_json::Value`
/// can: `node.into_deserializer()` instead of `NodeDeserializer::new(node)`.
impl<'de> IntoDeserializer<'de, Error> for Node<'de> {
    type Deserializer = NodeDeserializer<'de>;

    fn into_deserializer(self) -> Self::Deserializer {
        NodeDeserializer::new(self)
    }
}

impl<'de> de::Deserializer<'de> for NodeDeserializer<'de> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let origin = self.origin();
        self.deserialize_any_impl(visitor)
            .map_err(|err| origin.locate(err))
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let origin = self.origin();
        let is_null = matches!(self.node.value, Content::Empty)
            || self.node.tag == Tag::Standard(StandardTag::Null);
        if is_null {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
        .map_err(|err| origin.locate(err))
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let origin = self.origin();
        self.deserialize_enum_impl(name, variants, visitor)
            .map_err(|err| origin.locate(err))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

impl<'de> NodeDeserializer<'de> {
    /// [`deserialize_any`](de::Deserializer::deserialize_any) proper; its caller only adds the
    /// node's source location to whatever error comes back.
    fn deserialize_any_impl<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let source = self.source;
        let Node { tag, value, .. } = self.node;
        match (tag, value) {
            (_, Content::Empty) => visitor.visit_unit(),
            (_, Content::Seq(seq)) => visitor.visit_seq(SeqDeserializer::new(seq, source)),
            (_, Content::Map(map)) => visitor.visit_map(MapDeserializer::new(map, source)),
            (Tag::Standard(StandardTag::Null), Content::Scalar(_)) => visitor.visit_unit(),
            (Tag::Standard(StandardTag::Bool), Content::Scalar(scalar)) => {
                match scalar_text(&scalar) {
                    "true" | "True" | "TRUE" => visitor.visit_bool(true),
                    "false" | "False" | "FALSE" => visitor.visit_bool(false),
                    other => Err(Error::custom(format!("invalid bool lexeme: {other:?}"))),
                }
            }
            (Tag::Standard(StandardTag::Int), Content::Scalar(scalar)) => {
                let text = scalar_text(&scalar);
                if let Some(i) = parse_core_int(text) {
                    visitor.visit_i64(i)
                } else if let Some(u) = parse_core_uint(text) {
                    visitor.visit_u64(u)
                } else {
                    Err(Error::custom(format!(
                        "integer lexeme out of i64/u64 range: {text}"
                    )))
                }
            }
            (Tag::Standard(StandardTag::Float), Content::Scalar(scalar)) => {
                let text = scalar_text(&scalar);
                match parse_core_float(text) {
                    Some(f) => visitor.visit_f64(f),
                    None => Err(Error::custom(format!("invalid float lexeme: {text:?}"))),
                }
            }
            // Str, NonSpecific, Global (custom/unresolved tags), and defensively any other
            // tag/scalar combination that shouldn't occur once `resolve()` has run (Unspecified,
            // or a Standard(Map)/Standard(Seq) tag misapplied to a scalar) -- fall back to the raw
            // scalar text rather than erroring, since `NodeDeserializer` may also be constructed
            // directly on a hand-built, not-yet-resolved `Node`.
            (_, Content::Scalar(scalar)) => visit_scalar_str(scalar, visitor),
        }
    }

    /// [`deserialize_enum`](de::Deserializer::deserialize_enum) proper; see
    /// [`deserialize_any_impl`](Self::deserialize_any_impl).
    fn deserialize_enum_impl<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let source = self.source;
        match self.node.value {
            // A bare scalar names a unit variant directly (`variant: Foo`).
            Content::Scalar(scalar) => match scalar_cow(scalar) {
                Cow::Borrowed(s) => visitor.visit_enum(s.into_deserializer()),
                Cow::Owned(s) => visitor.visit_enum(s.into_deserializer()),
            },
            // Externally tagged representation: a single-entry mapping `{ Variant: value }`.
            Content::Map(mapping) => {
                let mut entries = mapping.0.into_iter();
                let entry = entries.next().ok_or_else(|| {
                    Error::custom(
                        "expected exactly one entry for an externally tagged enum, found an empty mapping",
                    )
                })?;
                if entries.next().is_some() {
                    return Err(Error::custom(
                        "expected exactly one entry for an externally tagged enum, found more than one",
                    ));
                }
                visitor.visit_enum(EnumDeserializer {
                    variant: entry.key,
                    value: entry.value,
                    source,
                })
            }
            other => Err(Error::custom(format!(
                "expected a string or a single-entry mapping for an enum, found {}",
                content_kind(&other)
            ))),
        }
    }
}

fn content_kind(content: &Content<'_>) -> &'static str {
    match content {
        Content::Empty => "null",
        Content::Scalar(_) => "a scalar",
        Content::Seq(_) => "a sequence",
        Content::Map(_) => "a mapping",
    }
}

fn scalar_text<'a>(scalar: &'a Scalar<'_>) -> &'a str {
    match scalar {
        Scalar::Plain(t)
        | Scalar::SingleStr(t)
        | Scalar::DoubleStr(t)
        | Scalar::Literal(t)
        | Scalar::Folded(t) => t,
    }
}

fn scalar_cow(scalar: Scalar<'_>) -> Cow<'_, str> {
    match scalar {
        Scalar::Plain(t)
        | Scalar::SingleStr(t)
        | Scalar::DoubleStr(t)
        | Scalar::Literal(t)
        | Scalar::Folded(t) => t,
    }
}

fn visit_scalar_str<'de, V>(scalar: Scalar<'de>, visitor: V) -> Result<V::Value, Error>
where
    V: Visitor<'de>,
{
    match scalar_cow(scalar) {
        Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
        Cow::Owned(s) => visitor.visit_string(s),
    }
}

/// [`EnumAccess`] for the externally tagged (single-entry mapping) enum representation.
struct EnumDeserializer<'de> {
    variant: Node<'de>,
    value: Node<'de>,
    source: Option<&'de str>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer<'de> {
    type Error = Error;
    type Variant = VariantDeserializer<'de>;

    fn variant_seed<S>(self, seed: S) -> Result<(S::Value, Self::Variant), Self::Error>
    where
        S: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(node_deserializer(self.variant, self.source))?;
        Ok((
            variant,
            VariantDeserializer {
                value: self.value,
                source: self.source,
            },
        ))
    }
}

struct VariantDeserializer<'de> {
    value: Node<'de>,
    source: Option<&'de str>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        let origin = Origin {
            span: self.value.span(),
            source: self.source,
        };
        match self.value.value {
            Content::Empty => Ok(()),
            other => Err(origin.locate(Error::custom(format!(
                "expected an empty value for a unit variant, found {}",
                content_kind(&other)
            )))),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(node_deserializer(self.value, self.source))
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_seq(node_deserializer(self.value, self.source), visitor)
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_map(node_deserializer(self.value, self.source), visitor)
    }
}

/// Builds the deserializer for a nested node, carrying the source down so it can locate its own
/// errors (see [`Origin`]).
fn node_deserializer<'de>(node: Node<'de>, source: Option<&'de str>) -> NodeDeserializer<'de> {
    match source {
        Some(source) => NodeDeserializer::with_source(node, source),
        None => NodeDeserializer::new(node),
    }
}

/// [`SeqAccess`] over an owned `Vec<Node>`.
struct SeqDeserializer<'de> {
    iter: std::vec::IntoIter<Node<'de>>,
    source: Option<&'de str>,
}

impl<'de> SeqDeserializer<'de> {
    fn new(seq: Vec<Node<'de>>, source: Option<&'de str>) -> Self {
        Self {
            iter: seq.into_iter(),
            source,
        }
    }
}

impl<'de> SeqAccess<'de> for SeqDeserializer<'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(node) => seed
                .deserialize(node_deserializer(node, self.source))
                .map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

/// [`MapAccess`] over an owned [`Mapping`].
struct MapDeserializer<'de> {
    iter: std::vec::IntoIter<MapEntry<'de>>,
    value: Option<Node<'de>>,
    source: Option<&'de str>,
}

impl<'de> MapDeserializer<'de> {
    fn new(mapping: Mapping<'de>, source: Option<&'de str>) -> Self {
        Self {
            iter: mapping.0.into_iter(),
            value: None,
            source,
        }
    }
}

impl<'de> MapAccess<'de> for MapDeserializer<'de> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(entry) => {
                self.value = Some(entry.value);
                seed.deserialize(node_deserializer(entry.key, self.source))
                    .map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .take()
            .expect("MapAccess::next_value_seed called without a preceding next_key_seed");
        seed.deserialize(node_deserializer(value, self.source))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Point {
        x: i64,
        y: i64,
    }

    #[test]
    fn deserializes_a_struct() {
        let point: Point = from_str("x: 1\ny: 2\n").unwrap();
        assert_eq!(point, Point { x: 1, y: 2 });
    }

    #[test]
    fn deserializes_primitives() {
        assert_eq!(from_str::<i64>("42\n").unwrap(), 42);
        assert_eq!(from_str::<f64>("3.5\n").unwrap(), 3.5);
        assert!(from_str::<bool>("true\n").unwrap());
        assert_eq!(from_str::<String>("hello\n").unwrap(), "hello");
        assert_eq!(from_str::<String>("\"hello\"\n").unwrap(), "hello");
    }

    #[test]
    fn deserializes_a_borrowed_str_zero_copy() {
        // A plain scalar's text is borrowed straight from the input; assert the round trip works
        // through the zero-copy `visit_borrowed_str` path (not just `visit_string`).
        let s: &str = from_str("hello\n").unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn deserializes_option() {
        assert_eq!(from_str::<Option<i64>>("~\n").unwrap(), None);
        assert_eq!(from_str::<Option<i64>>("\n").unwrap(), None);
        assert_eq!(from_str::<Option<i64>>("5\n").unwrap(), Some(5));
    }

    #[test]
    fn empty_stream_deserializes_as_null() {
        assert_eq!(from_str::<Option<i64>>("").unwrap(), None);
    }

    #[test]
    fn multi_document_stream_deserializes_one_document_at_a_time() {
        // The escape hatch `from_str`'s own error points at: one `T` per document, no manual
        // `NodeDeserializer` loop needed.
        let points: Vec<Point> = Deserializer::from_str("x: 1\ny: 2\n---\nx: 3\ny: 4\n")
            .into_iter()
            .collect::<crate::Result<_>>()
            .unwrap();
        assert_eq!(points, vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
    }

    /// An empty stream has no documents at all -- unlike `from_str`, which reads one absent
    /// document as null.
    #[test]
    fn stream_deserializer_is_empty_for_an_empty_input() {
        assert_eq!(Deserializer::from_str("").into_iter::<Point>().count(), 0);
    }

    #[test]
    fn stream_deserializer_surfaces_per_document_errors() {
        let mut stream = Deserializer::from_str("x: 1\ny: 2\n---\nnope\n").into_iter::<Point>();
        assert_eq!(stream.next().unwrap().unwrap(), Point { x: 1, y: 2 });
        assert!(matches!(
            stream.next().unwrap(),
            Err(Error::Deserialize { .. })
        ));
        assert!(stream.next().is_none());
    }

    /// The whole point of the lazy parse layer: the first document is deserialized without the
    /// later, unparseable one having been looked at.
    #[test]
    fn stream_deserializer_yields_a_value_before_parsing_a_later_broken_document() {
        let mut stream = Deserializer::from_str("x: 1\ny: 2\n---\n[unclosed").into_iter::<Point>();
        assert_eq!(stream.next().unwrap().unwrap(), Point { x: 1, y: 2 });
    }

    #[test]
    fn deserializes_a_node_via_into_deserializer() {
        let node = crate::parse_document("x: 1\ny: 2\n").unwrap().into_node();
        assert_eq!(
            Point::deserialize(node.into_deserializer()).unwrap(),
            Point { x: 1, y: 2 }
        );
    }

    #[test]
    fn deserializes_from_bytes() {
        assert_eq!(
            from_bytes::<Point>(b"x: 1\ny: 2\n").unwrap(),
            Point { x: 1, y: 2 }
        );
        assert!(matches!(
            from_bytes::<Point>(&[0xff, 0xfe]).unwrap_err(),
            Error::Utf8(_)
        ));
    }

    #[test]
    fn deserializes_a_sequence() {
        assert_eq!(
            from_str::<Vec<i64>>("- 1\n- 2\n- 3\n").unwrap(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn deserializes_a_map() {
        let map: BTreeMap<String, i64> = from_str("a: 1\nb: 2\n").unwrap();
        assert_eq!(
            map,
            BTreeMap::from([("a".to_string(), 1), ("b".to_string(), 2)])
        );
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    enum Shape {
        Point,
        Circle(f64),
        Rectangle { width: f64, height: f64 },
    }

    #[test]
    fn deserializes_externally_tagged_enum_variants() {
        assert_eq!(from_str::<Shape>("Point\n").unwrap(), Shape::Point);
        assert_eq!(
            from_str::<Shape>("Circle: 1.5\n").unwrap(),
            Shape::Circle(1.5)
        );
        assert_eq!(
            from_str::<Shape>("Rectangle:\n  width: 2\n  height: 3\n").unwrap(),
            Shape::Rectangle {
                width: 2.0,
                height: 3.0
            }
        );
    }

    #[test]
    fn rejects_multiple_documents() {
        let err = from_str::<i64>("---\n1\n---\n2\n").unwrap_err();
        assert_eq!(err, Error::MultipleDocuments);
    }

    #[test]
    fn reports_type_mismatch_as_deserialize_error() {
        let err = from_str::<i64>("not a number\n").unwrap_err();
        assert!(matches!(err, Error::Deserialize { .. }));
    }

    /// A failure inside a nested value points at *that* value, not at the whole document: the
    /// innermost deserializer still running attaches its node's span first.
    #[test]
    fn locates_a_type_mismatch_at_the_offending_node() {
        let err = from_str::<Point>("x: 1\ny: nope\n").unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("y: nope"), "{rendered}");
        assert!(rendered.contains("2 |"), "{rendered}");
    }

    /// serde raises this one from the visitor, with no node of its own -- it lands on the mapping
    /// that was missing the field.
    #[test]
    fn locates_a_missing_field_at_the_mapping() {
        let err = from_str::<Point>("x: 1\n").unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("missing field `y`"), "{rendered}");
        assert!(rendered.contains("x: 1"), "{rendered}");
    }

    /// One of the three exact-output tests -- see `crate::error`'s own for the rationale.
    #[test]
    fn deserialize_error_renders_the_offending_source() {
        let err = from_str::<Point>("x: 1\ny: nope\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            "\
error: invalid type: string \"nope\", expected i64
  |
2 | y: nope
  |    ^^^^"
        );
    }

    /// Without a source there is nothing to render an excerpt against, so the message stands
    /// alone -- the node still knows its span, but a `Node` alone doesn't carry the text.
    #[test]
    fn node_deserializer_without_a_source_reports_an_unlocated_error() {
        let node = crate::parse_document("nope\n").unwrap().into_node();
        assert!(node.span().is_some());
        let err = i64::deserialize(NodeDeserializer::new(node)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid type: string \"nope\", expected i64"
        );
    }
}
