//! Defines several contexts.
//!
//! https://yaml.org/spec/1.2.2/#context-c

use std::borrow::Cow;

use winnow::{combinator::trace, Parser};

use super::{
    double,
    error::ParserError,
    input::InputStream,
    plain, single,
    spaces::{self, IndentLevel},
};

/// Parse context to differentiate behaviors.
#[allow(private_bounds)]
pub trait YamlContext: Sealed + Clone + Copy {
    /// Returns instance.
    fn get() -> Self;

    /// s-separate implementation.
    /// See [`super::spaces::separate()`] for its impl.
    fn separate<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error, Self>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>;
}

/// Non-key contexts, which is
/// * [`BlockIn`]
/// * [`BlockOut`]
/// * [`FlowIn`]
/// * [`FlowOut`]
pub trait NonKey: YamlContext {
    /// s-line-prefix implementation.
    fn line_prefix<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error, Self>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>;
}

/// Context which is in/out of flow.
/// * [`FlowIn`]
/// * [`FlowOut`]
pub trait InOutFlow: NonKey {}

/// Context which is in/out of block.
/// * [`BlockIn`]
/// * [`BlockOut`]
pub trait InOutBlock: NonKey {}

/// Flow in/out context or key context.
/// * [`BlockKey`]
/// * [`FlowKey`]
pub trait Key: FlowOrKey {}

/// In the flow context, which is [`FlowIn`] or [`FlowKey`].
pub trait InFlow: FlowOrKey {}

pub trait FlowOrKey: YamlContext {
    type Flow: InFlow;

    /// Returns true for ns-plain-safe character.
    ///
    /// https://yaml.org/spec/1.2.2/#rule-ns-plain-safe
    #[doc(alias = "ns-plain-safe")]
    fn is_plain_safe(c: char) -> bool;

    /// implementation of nb-double-text.
    fn non_break_double_text<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>;

    /// implementation of nb-single-text.
    fn non_break_single_text<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>;

    /// implementation of ns-plain.
    fn non_space_plain<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>;
}

trait Sealed {}

/// BLOCK-IN context.
#[derive(Debug, Clone, Copy)]
pub struct BlockIn;

/// BLOCK-OUT context.
#[derive(Debug, Clone, Copy)]
pub struct BlockOut;

/// BLOCK-KEY context.
#[derive(Debug, Clone, Copy)]
pub struct BlockKey;

/// FLOW-IN context.
#[derive(Debug, Clone, Copy)]
pub struct FlowIn;

/// FLOW-OUT context.
#[derive(Debug, Clone, Copy)]
pub struct FlowOut;

/// FLOW-KEY context.
#[derive(Debug, Clone, Copy)]
pub struct FlowKey;

impl Sealed for BlockIn {}
impl Sealed for BlockOut {}
impl Sealed for BlockKey {}
impl Sealed for FlowIn {}
impl Sealed for FlowOut {}
impl Sealed for FlowKey {}

impl InOutFlow for FlowIn {}
impl InOutFlow for FlowOut {}

impl InOutBlock for BlockIn {}
impl InOutBlock for BlockOut {}

impl InFlow for FlowIn {}
impl InFlow for FlowKey {}

impl Key for BlockKey {}
impl Key for FlowKey {}

impl YamlContext for BlockIn {
    fn get() -> Self {
        Self
    }

    fn separate<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        trace(
            "spaces::separate(BLOCK-IN)",
            spaces::separate_lines(indent_level),
        )
    }
}

impl YamlContext for BlockOut {
    fn get() -> Self {
        Self
    }

    fn separate<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        trace(
            "spaces::separate(BLOCK-OUT)",
            spaces::separate_lines(indent_level),
        )
    }
}

impl YamlContext for FlowIn {
    fn get() -> Self {
        Self
    }

    fn separate<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        trace(
            "spaces::separate(FLOW-IN)",
            spaces::separate_lines(indent_level),
        )
    }
}

impl YamlContext for FlowOut {
    fn get() -> Self {
        Self
    }

    fn separate<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        trace(
            "spaces::separate(FLOW-OUT)",
            spaces::separate_lines(indent_level),
        )
    }
}

impl YamlContext for BlockKey {
    fn get() -> Self {
        Self
    }

    fn separate<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        trace("spaces::separate(BLOCK-KEY)", spaces::separate_in_line)
    }
}

impl YamlContext for FlowKey {
    fn get() -> Self {
        Self
    }

    fn separate<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, (), Error> + use<'i, Input, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        trace("spaces::separate(FLOW-KEY)", spaces::separate_in_line)
    }
}

impl FlowOrKey for FlowOut {
    type Flow = FlowIn;

    fn is_plain_safe(c: char) -> bool {
        plain::is_plain_safe_out(c)
    }

    fn non_break_double_text<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        double::non_break_double_multi_line(indent_level)
    }

    fn non_break_single_text<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        single::non_break_single_multi_line(indent_level)
    }

    fn non_space_plain<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        plain::non_space_plain_multi_line(indent_level)
    }
}

impl FlowOrKey for FlowIn {
    type Flow = FlowIn;

    fn is_plain_safe(c: char) -> bool {
        plain::is_plain_safe_in(c)
    }

    fn non_break_double_text<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        double::non_break_double_multi_line(indent_level)
    }

    fn non_break_single_text<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        single::non_break_single_multi_line(indent_level)
    }

    fn non_space_plain<'i, Input, Error>(
        indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        plain::non_space_plain_multi_line(indent_level)
    }
}

impl FlowOrKey for FlowKey {
    type Flow = FlowKey;

    fn is_plain_safe(c: char) -> bool {
        plain::is_plain_safe_in(c)
    }

    fn non_break_double_text<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        double::non_break_double_one_line
    }

    fn non_break_single_text<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        single::non_break_single_one_line
    }

    fn non_space_plain<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        plain::non_space_plain_one_line
    }
}

impl FlowOrKey for BlockKey {
    type Flow = FlowKey;

    fn is_plain_safe(c: char) -> bool {
        plain::is_plain_safe_out(c)
    }

    fn non_break_double_text<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        double::non_break_double_one_line
    }

    fn non_break_single_text<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        single::non_break_single_one_line
    }

    fn non_space_plain<'i, Input, Error>(
        _indent_level: IndentLevel,
    ) -> impl Parser<Input, Cow<'i, str>, Error>
    where
        Input: InputStream<'i>,
        Error: ParserError<Input>,
    {
        plain::non_space_plain_one_line
    }
}
