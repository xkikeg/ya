//! Optional [`serde::Deserialize`] support (Cargo feature `serde`), completing the Construct phase
//! this crate's design has always deferred to it -- see AGENT.md Phase 6's design note ("typed
//! accessors on demand ... and on top of those the Phase 8 serde `Deserialize` layer -- both parse
//! the retained lexeme against the caller's requested type at conversion time").
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
//! Only `Deserialize` is implemented, not `Serialize`: the tag-only resolution model (AGENT.md
//! Phase 6) makes a `Node -> T` conversion a natural, lossy-by-design projection (an `!!int`'s
//! text is parsed against whatever integer type the caller asks for), whereas `T -> Node` would
//! need to invent presentation-layer decisions (style, quoting) this crate doesn't model at all.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{
    self, Deserialize, DeserializeSeed, EnumAccess, Error as _, IntoDeserializer, MapAccess,
    SeqAccess, VariantAccess, Visitor,
};

use crate::value::{parse_core_float, parse_core_int, parse_core_uint};
use crate::value::{Content, Document, MapEntry, Mapping, Node, Scalar, StandardTag, Stream, Tag};
use crate::Error;

/// Deserializes `input` as YAML into `T`.
///
/// An empty stream (`input` containing zero documents, e.g. `""` or a comment-only file)
/// deserializes as an implicit null node, matching the Core Schema's own treatment of an empty
/// document (AGENT.md Phase 6's resolution table). A stream with more than one document is
/// rejected -- use [`Deserializer::into_iter`] for those.
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
    T::deserialize(Deserializer::from_str(input)?)
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
        Error::Custom(msg.to_string())
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
///     .unwrap()
///     .into_iter()
///     .collect::<Result<_, _>>()
///     .unwrap();
/// assert_eq!(points, vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
/// ```
pub struct Deserializer<'de> {
    stream: Stream<'de>,
}

impl<'de> Deserializer<'de> {
    /// Parses `input` (representation parsing + Core Schema resolution, i.e. [`crate::parse`])
    /// into a deserializer over the resulting stream.
    // Not `std::str::FromStr`: that trait can't borrow from its input, which is the entire point
    // of this type. Named `from_str` to match `serde_json::Deserializer::from_str`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &'de str) -> crate::Result<Self> {
        Ok(Self {
            stream: crate::parse(input)?,
        })
    }

    /// Like [`from_str`](Self::from_str), for UTF-8 encoded input.
    pub fn from_bytes(input: &'de [u8]) -> crate::Result<Self> {
        Self::from_str(std::str::from_utf8(input).map_err(Error::Utf8)?)
    }

    /// Turns this into an iterator yielding one `T` per document in the stream.
    ///
    /// Unlike `serde_json::StreamDeserializer` -- which must stream, since JSON has no document
    /// separator -- [`crate::parse`] has already materialized every document by this point, so
    /// this iterates over parsed documents rather than parsing lazily. The API shape matches;
    /// the laziness doesn't.
    // Not `IntoIterator`: the element type is chosen by the caller (`into_iter::<T>()`), which a
    // trait impl can't express. Same signature as `serde_json::Deserializer::into_iter`.
    #[allow(clippy::should_implement_trait)]
    pub fn into_iter<T>(self) -> StreamDeserializer<'de, T>
    where
        T: Deserialize<'de>,
    {
        StreamDeserializer {
            documents: self.stream.into_documents().into_iter(),
            marker: PhantomData,
        }
    }

    /// Reduces the stream to the single node to deserialize, per [`from_str`]'s documented rules.
    fn into_single(self) -> crate::Result<NodeDeserializer<'de>> {
        let mut documents = self.stream.into_documents().into_iter();
        let node = match documents.next() {
            None => Node::new(Content::Empty, Tag::Standard(StandardTag::Null)),
            Some(doc) => doc.into_node(),
        };
        if documents.next().is_some() {
            return Err(Error::custom(
                "expected a single YAML document, found more than one; use `Deserializer::into_iter` for a multi-document stream",
            ));
        }
        Ok(NodeDeserializer::new(node))
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

/// An iterator over the documents of a [`Deserializer`], deserializing each into a `T`.
///
/// Created by [`Deserializer::into_iter`]; see there for how this differs from
/// `serde_json::StreamDeserializer`.
pub struct StreamDeserializer<'de, T> {
    documents: std::vec::IntoIter<Document<'de>>,
    marker: PhantomData<T>,
}

impl<'de, T> Iterator for StreamDeserializer<'de, T>
where
    T: Deserialize<'de>,
{
    type Item = crate::Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.documents
            .next()
            .map(|doc| T::deserialize(NodeDeserializer::new(doc.into_node())))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.documents.len();
        (remaining, Some(remaining))
    }
}

impl<'de, T> ExactSizeIterator for StreamDeserializer<'de, T>
where
    T: Deserialize<'de>,
{
    fn len(&self) -> usize {
        self.documents.len()
    }
}

/// A [`serde::Deserializer`] over an owned [`Node`]. See the module docs for why it consumes
/// rather than borrows its node.
pub struct NodeDeserializer<'de> {
    node: Node<'de>,
}

impl<'de> NodeDeserializer<'de> {
    /// Creates a new instance wrapping `node`.
    pub fn new(node: Node<'de>) -> Self {
        Self { node }
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
        let Node { tag, value } = self.node;
        match (tag, value) {
            (_, Content::Empty) => visitor.visit_unit(),
            (_, Content::Seq(seq)) => visitor.visit_seq(SeqDeserializer::new(seq)),
            (_, Content::Map(map)) => visitor.visit_map(MapDeserializer::new(map)),
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

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let is_null = matches!(self.node.value, Content::Empty)
            || self.node.tag == Tag::Standard(StandardTag::Null);
        if is_null {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
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
                })
            }
            other => Err(Error::custom(format!(
                "expected a string or a single-entry mapping for an enum, found {}",
                content_kind(&other)
            ))),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
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
}

impl<'de> EnumAccess<'de> for EnumDeserializer<'de> {
    type Error = Error;
    type Variant = VariantDeserializer<'de>;

    fn variant_seed<S>(self, seed: S) -> Result<(S::Value, Self::Variant), Self::Error>
    where
        S: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(NodeDeserializer::new(self.variant))?;
        Ok((variant, VariantDeserializer { value: self.value }))
    }
}

struct VariantDeserializer<'de> {
    value: Node<'de>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.value.value {
            Content::Empty => Ok(()),
            other => Err(Error::custom(format!(
                "expected an empty value for a unit variant, found {}",
                content_kind(&other)
            ))),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(NodeDeserializer::new(self.value))
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_seq(NodeDeserializer::new(self.value), visitor)
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_map(NodeDeserializer::new(self.value), visitor)
    }
}

/// [`SeqAccess`] over an owned `Vec<Node>`.
struct SeqDeserializer<'de> {
    iter: std::vec::IntoIter<Node<'de>>,
}

impl<'de> SeqDeserializer<'de> {
    fn new(seq: Vec<Node<'de>>) -> Self {
        Self {
            iter: seq.into_iter(),
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
            Some(node) => seed.deserialize(NodeDeserializer::new(node)).map(Some),
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
}

impl<'de> MapDeserializer<'de> {
    fn new(mapping: Mapping<'de>) -> Self {
        Self {
            iter: mapping.0.into_iter(),
            value: None,
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
                seed.deserialize(NodeDeserializer::new(entry.key)).map(Some)
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
        seed.deserialize(NodeDeserializer::new(value))
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
            .unwrap()
            .into_iter()
            .collect::<crate::Result<_>>()
            .unwrap();
        assert_eq!(points, vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
    }

    #[test]
    fn stream_deserializer_reports_its_document_count() {
        let stream = Deserializer::from_str("x: 1\ny: 2\n---\nx: 3\ny: 4\n")
            .unwrap()
            .into_iter::<Point>();
        assert_eq!(stream.len(), 2);
        // An empty stream has no documents at all -- unlike `from_str`, which reads one absent
        // document as null.
        assert_eq!(
            Deserializer::from_str("")
                .unwrap()
                .into_iter::<Point>()
                .len(),
            0
        );
    }

    #[test]
    fn stream_deserializer_surfaces_per_document_errors() {
        let mut stream = Deserializer::from_str("x: 1\ny: 2\n---\nnope\n")
            .unwrap()
            .into_iter::<Point>();
        assert_eq!(stream.next().unwrap().unwrap(), Point { x: 1, y: 2 });
        assert!(matches!(stream.next().unwrap(), Err(Error::Custom(_))));
        assert!(stream.next().is_none());
    }

    #[test]
    fn deserializes_a_node_via_into_deserializer() {
        let stream = crate::parse("x: 1\ny: 2\n").unwrap();
        let node = stream.into_documents().pop().unwrap().into_node();
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
        assert!(matches!(err, Error::Custom(_)));
    }

    #[test]
    fn reports_type_mismatch_as_custom_error() {
        let err = from_str::<i64>("not a number\n").unwrap_err();
        assert!(matches!(err, Error::Custom(_)));
    }
}
