//! Defines values result as a YAML parse.

use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub struct Stream<'i>(pub(crate) Vec<Document<'i>>);

impl<'i> Stream<'i> {
    /// Takes the reference to the documents in the stream.
    pub fn documents(&self) -> &[Document<'i>] {
        &self.0
    }

    /// Consumes the stream and returns the documents it holds, for callers who need owned
    /// [`Node`]s rather than the borrows [`documents`](Self::documents) hands out.
    ///
    /// A `Stream` comes from [`crate::parse::yaml_stream`], which parses every document up front;
    /// [`crate::parse_stream`] yields the same documents one at a time instead.
    ///
    /// ```
    /// use winnow::Parser as _;
    ///
    /// let stream = ya::parse::yaml_stream::<_, winnow::error::ContextError>
    ///     .parse(ya::parse::input::Input::new("a: 1\n---\na: 2\n"))
    ///     .unwrap();
    /// let values: Vec<i64> = ya::resolve::resolve(stream)
    ///     .unwrap()
    ///     .into_documents()
    ///     .into_iter()
    ///     .map(|doc| {
    ///         let ya::value::Content::Map(map) = doc.into_node().value else {
    ///             panic!("expected a mapping");
    ///         };
    ///         map.entries()[0].value.as_i64().unwrap()
    ///     })
    ///     .collect();
    /// assert_eq!(values, vec![1, 2]);
    /// ```
    pub fn into_documents(self) -> Vec<Document<'i>> {
        self.0
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

    /// This node's scalar text, if it's a scalar (or empty) node; `None` for a sequence/mapping.
    fn scalar_text(&self) -> Option<&str> {
        match &self.value {
            Content::Empty => Some(""),
            Content::Scalar(
                Scalar::Plain(t)
                | Scalar::SingleStr(t)
                | Scalar::DoubleStr(t)
                | Scalar::Literal(t)
                | Scalar::Folded(t),
            ) => Some(t),
            Content::Seq(_) | Content::Map(_) => None,
        }
    }

    /// Returns `true` if this node's tag is `tag:yaml.org,2002:null`. Only meaningful once the
    /// stream has gone through [`crate::resolve::resolve`]: before that, an untagged null scalar
    /// (`~`, `null`, or simply empty) still carries [`Tag::Unspecified`].
    pub fn is_null(&self) -> bool {
        self.tag == Tag::Standard(StandardTag::Null)
    }

    /// Interprets this node as a bool, if its tag is `tag:yaml.org,2002:bool` (see
    /// [`Self::is_null`]'s note on calling this after [`crate::resolve::resolve`]).
    pub fn as_bool(&self) -> Option<bool> {
        if self.tag != Tag::Standard(StandardTag::Bool) {
            return None;
        }
        match self.scalar_text()? {
            "true" | "True" | "TRUE" => Some(true),
            "false" | "False" | "FALSE" => Some(false),
            _ => None,
        }
    }

    /// Interprets this node as a string: `tag:yaml.org,2002:str`, or the non-specific tag `!`
    /// (which forces str/map/seq by kind without further Core Schema resolution -- see
    /// [`crate::resolve`]'s module docs for why `resolve()` never rewrites it away). See
    /// [`Self::is_null`]'s note on calling this after [`crate::resolve::resolve`].
    pub fn as_str(&self) -> Option<&str> {
        match self.tag {
            Tag::Standard(StandardTag::Str) | Tag::NonSpecific => self.scalar_text(),
            _ => None,
        }
    }

    /// Interprets this node as an `i64`, parsing the decimal/octal (`0o`)/hex (`0x`) forms per the
    /// Core Schema, if its tag is `tag:yaml.org,2002:int` (see [`Self::is_null`]'s note on calling
    /// this after [`crate::resolve::resolve`]). Returns `None` -- rather than erroring -- both
    /// when the tag doesn't match and when the value overflows `i64`: the node itself stays valid
    /// either way, only this particular conversion fails.
    pub fn as_i64(&self) -> Option<i64> {
        if self.tag != Tag::Standard(StandardTag::Int) {
            return None;
        }
        parse_core_int(self.scalar_text()?)
    }

    /// Interprets this node as an `f64`, if its tag is `tag:yaml.org,2002:float` (see
    /// [`Self::is_null`]'s note on calling this after [`crate::resolve::resolve`]).
    pub fn as_f64(&self) -> Option<f64> {
        if self.tag != Tag::Standard(StandardTag::Float) {
            return None;
        }
        parse_core_float(self.scalar_text()?)
    }
}

/// Parses a Core Schema int lexeme (`[-+]?[0-9]+`, `0o[0-7]+`, or `0x[0-9a-fA-F]+`) already known
/// to match [`crate::resolve`]'s classifier. `None` on `i64` overflow.
pub(crate) fn parse_core_int(text: &str) -> Option<i64> {
    let (sign, rest): (i64, &str) = match text.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let magnitude = if let Some(hex) = rest.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()?
    } else if let Some(oct) = rest.strip_prefix("0o") {
        i64::from_str_radix(oct, 8).ok()?
    } else {
        rest.parse::<i64>().ok()?
    };
    magnitude.checked_mul(sign)
}

/// Like [`parse_core_int`], but for magnitudes that fit `u64` but not `i64` (a positive int lexeme
/// with no `-`/`+` sign wider than `i64::MAX`). Used by the `serde` Deserializer (`crate::de`) as
/// a fallback for `deserialize_any`/`visit_u64` when [`parse_core_int`] overflows.
#[cfg(feature = "serde")]
pub(crate) fn parse_core_uint(text: &str) -> Option<u64> {
    if let Some(hex) = text.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else if let Some(oct) = text.strip_prefix("0o") {
        u64::from_str_radix(oct, 8).ok()
    } else {
        text.parse::<u64>().ok()
    }
}

/// Parses a Core Schema float lexeme already known to match [`crate::resolve`]'s classifier:
/// `.inf`/`.Inf`/`.INF` (optionally signed), `.nan`/`.NaN`/`.NAN`, or an ordinary decimal float
/// that [`str::parse`] already understands.
pub(crate) fn parse_core_float(text: &str) -> Option<f64> {
    if matches!(text, ".nan" | ".NaN" | ".NAN") {
        return Some(f64::NAN);
    }
    let (sign, rest) = match text.strip_prefix('-') {
        Some(r) => (-1.0, r),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    if matches!(rest, ".inf" | ".Inf" | ".INF") {
        return Some(sign * f64::INFINITY);
    }
    text.parse::<f64>().ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(tag: StandardTag, text: &str) -> Node<'_> {
        Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed(text))),
            Tag::Standard(tag),
        )
    }

    #[test]
    fn is_null_requires_standard_null_tag() {
        assert!(tagged(StandardTag::Null, "null").is_null());
        assert!(!tagged(StandardTag::Str, "null").is_null());
        assert!(!Node::unspecified(Content::Empty).is_null());
    }

    #[test]
    fn as_bool_parses_all_core_schema_forms() {
        for text in ["true", "True", "TRUE"] {
            assert_eq!(Some(true), tagged(StandardTag::Bool, text).as_bool());
        }
        for text in ["false", "False", "FALSE"] {
            assert_eq!(Some(false), tagged(StandardTag::Bool, text).as_bool());
        }
        assert_eq!(None, tagged(StandardTag::Str, "true").as_bool());
    }

    #[test]
    fn as_str_accepts_standard_str_and_non_specific() {
        assert_eq!(Some("foo"), tagged(StandardTag::Str, "foo").as_str());
        let non_specific = Node::new(
            Content::Scalar(Scalar::Plain(Cow::Borrowed("foo"))),
            Tag::NonSpecific,
        );
        assert_eq!(Some("foo"), non_specific.as_str());
        assert_eq!(None, tagged(StandardTag::Int, "42").as_str());
    }

    #[test]
    fn as_i64_parses_decimal_octal_and_hex() {
        assert_eq!(Some(7), tagged(StandardTag::Int, "0o7").as_i64());
        assert_eq!(Some(0x1A), tagged(StandardTag::Int, "0x1A").as_i64());
        assert_eq!(Some(-12), tagged(StandardTag::Int, "-12").as_i64());
        assert_eq!(Some(12), tagged(StandardTag::Int, "+12").as_i64());
    }

    #[test]
    fn as_i64_overflow_leaves_node_valid_but_fails_conversion() {
        let node = tagged(StandardTag::Int, "99999999999999999999999");
        assert_eq!(None, node.as_i64());
        // The node itself is untouched: its tag and text are still exactly what was constructed.
        assert_eq!(Tag::Standard(StandardTag::Int), node.tag);
        assert_eq!(Some("99999999999999999999999"), node.scalar_text());
    }

    #[test]
    fn as_f64_parses_regular_and_special_forms() {
        assert_eq!(Some(3.5), tagged(StandardTag::Float, "3.5").as_f64());
        assert_eq!(
            Some(f64::INFINITY),
            tagged(StandardTag::Float, ".inf").as_f64()
        );
        assert_eq!(
            Some(f64::NEG_INFINITY),
            tagged(StandardTag::Float, "-.inf").as_f64()
        );
        assert!(tagged(StandardTag::Float, ".nan")
            .as_f64()
            .unwrap()
            .is_nan());
    }
}
