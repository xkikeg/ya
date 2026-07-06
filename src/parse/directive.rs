//! Directives: `%YAML`, `%TAG`, and reserved (unrecognized) directives.
//!
//! https://yaml.org/spec/1.2.2/#68-directives

use std::borrow::Cow;

use winnow::{
    combinator::{alt, opt, peek, preceded, repeat, terminated, trace},
    token::{one_of, take_while},
    Parser,
};

use super::{chars, error::ParserError, input::InputStream, properties, spaces};

/// A single parsed directive. Only `%YAML` and `%TAG` carry semantic content that the caller
/// (`document::directive_document`) must act on (version check / duplicate detection, and
/// registering the handle -> prefix mapping respectively); reserved directives have no known
/// semantics and are consumed and ignored, per spec prose.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Directive<'i> {
    Yaml {
        major: u32,
        minor: u32,
    },
    Tag {
        handle: &'i str,
        prefix: Cow<'i, str>,
    },
    Reserved,
}

/// A directive line: `%` followed by a YAML/TAG/reserved directive, then trailing comments.
///
/// https://yaml.org/spec/1.2.2/#rule-l-directive
#[doc(alias = "l-directive")]
pub(super) fn directive<'i, Input, Error>(input: &mut Input) -> winnow::Result<Directive<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::directive",
        terminated(preceded('%', directive_body), spaces::line_comments),
    )
    .parse_next(input)
}

/// Dispatches on the directive name (peeked, not consumed) to decide which of
/// [`yaml_directive`]/[`tag_directive`]/[`reserved_directive`] to commit to.
///
/// Deliberately not a plain `alt((yaml_directive, tag_directive, reserved_directive))`: since
/// `reserved_directive`'s name grammar (`ns-char+`) also matches the literal `"YAML"`/`"TAG"`,
/// a backtracking `alt` would silently reinterpret a *malformed* `%YAML`/`%TAG` body (e.g. `%YAML
/// 2.0`, an unsupported major version) as an unrelated reserved directive instead of rejecting it
/// -- once the name is unambiguously `YAML` or `TAG`, any failure from here on must be a hard
/// parse error, not a fall-through.
fn directive_body<'i, Input, Error>(input: &mut Input) -> winnow::Result<Directive<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::directive_body",
        move |input: &mut Input| match peek(directive_name).parse_next(input)? {
            "YAML" => yaml_directive(input),
            "TAG" => tag_directive(input),
            _ => reserved_directive(input),
        },
    )
    .parse_next(input)
}

/// `%YAML` directive: declares the document's YAML version. Only major version 1 is accepted (a
/// hard parse error otherwise, since this crate only implements 1.2.2). The spec says a processor
/// "should" warn when the minor version isn't 2, but there's no warning channel in this crate, so
/// any minor version is silently accepted and treated as 1.2. Called only once [`directive_body`]
/// has already confirmed the directive name is exactly `"YAML"`, so a bad version here is a hard
/// error, not a reason to try reinterpreting the line as something else.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-yaml-directive
#[doc(alias = "ns-yaml-directive")]
fn yaml_directive<'i, Input, Error>(input: &mut Input) -> winnow::Result<Directive<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::yaml_directive",
        preceded(("YAML", spaces::separate_in_line), yaml_version)
            .verify(|(major, _)| *major == 1)
            .map(|(major, minor)| Directive::Yaml { major, minor }),
    )
    .parse_next(input)
}

/// `ns-yaml-version`: `ns-dec-digit+ "." ns-dec-digit+`, parsed as `(major, minor)`.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-yaml-version
#[doc(alias = "ns-yaml-version")]
fn yaml_version<'i, Input, Error>(input: &mut Input) -> winnow::Result<(u32, u32), Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::yaml_version",
        (dec_digits, '.', dec_digits).try_map(
            |(major, _, minor): (&str, char, &str)| -> Result<(u32, u32), std::num::ParseIntError> {
                Ok((major.parse()?, minor.parse()?))
            },
        ),
    )
    .parse_next(input)
}

/// `ns-dec-digit+`, used by [`yaml_version`].
///
/// https://yaml.org/spec/1.2.2/#rule-ns-dec-digit
#[doc(alias = "ns-dec-digit")]
fn dec_digits<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::dec_digits",
        take_while(1.., |c: char| c.is_ascii_digit()),
    )
    .parse_next(input)
}

/// `%TAG` directive: a tag handle and the prefix it expands to. Registering the mapping (and
/// rejecting a handle declared twice in the same document) is the caller's job
/// (`document::directive_document`), since that requires mutating the parse-time tag-handle map.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-tag-directive
#[doc(alias = "ns-tag-directive")]
fn tag_directive<'i, Input, Error>(input: &mut Input) -> winnow::Result<Directive<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::tag_directive",
        preceded(
            ("TAG", spaces::separate_in_line),
            (
                terminated(properties::tag_handle, spaces::separate_in_line),
                tag_prefix,
            ),
        )
        .map(|(handle, prefix)| Directive::Tag { handle, prefix }),
    )
    .parse_next(input)
}

/// `ns-tag-prefix`: a local tag prefix (`!` + optional URI chars) or a global one (URI chars not
/// starting with `!`).
///
/// https://yaml.org/spec/1.2.2/#rule-ns-tag-prefix
#[doc(alias = "ns-tag-prefix")]
fn tag_prefix<'i, Input, Error>(input: &mut Input) -> winnow::Result<Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::tag_prefix",
        alt((local_tag_prefix, global_tag_prefix)).map(Cow::Borrowed),
    )
    .parse_next(input)
}

/// `c-ns-local-tag-prefix`: `!` followed by zero or more URI chars.
///
/// https://yaml.org/spec/1.2.2/#rule-c-ns-local-tag-prefix
#[doc(alias = "c-ns-local-tag-prefix")]
fn local_tag_prefix<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::local_tag_prefix",
        (one_of('!'), opt(chars::uri_chars)).take(),
    )
    .parse_next(input)
}

/// `ns-global-tag-prefix`: a tag char followed by zero or more URI chars.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-global-tag-prefix
#[doc(alias = "ns-global-tag-prefix")]
fn global_tag_prefix<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::global_tag_prefix",
        (one_of(chars::is_tag_char), opt(chars::uri_chars)).take(),
    )
    .parse_next(input)
}

/// `ns-reserved-directive`: an unrecognized directive name plus optional parameters. Consumed and
/// ignored -- there's no known semantics to apply, and the spec only asks that a processor accept
/// (optionally with a warning) rather than reject these.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-reserved-directive
#[doc(alias = "ns-reserved-directive")]
fn reserved_directive<'i, Input, Error>(input: &mut Input) -> winnow::Result<Directive<'i>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::reserved_directive",
        (
            directive_name,
            repeat(0.., preceded(spaces::separate_in_line, directive_parameter)).map(|()| ()),
        )
            .value(Directive::Reserved),
    )
    .parse_next(input)
}

/// `ns-directive-name`: `ns-char+`.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-directive-name
#[doc(alias = "ns-directive-name")]
fn directive_name<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::directive_name",
        take_while(1.., chars::is_non_space),
    )
    .parse_next(input)
}

/// `ns-directive-parameter`: `ns-char+`.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-directive-parameter
#[doc(alias = "ns-directive-parameter")]
fn directive_parameter<'i, Input, Error>(input: &mut Input) -> winnow::Result<&'i str, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "directive::directive_parameter",
        take_while(1.., chars::is_non_space),
    )
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::testing;

    #[test]
    fn yaml_directive_1_2() {
        assert_eq!(
            ("", Directive::Yaml { major: 1, minor: 2 }),
            testing::parse(directive, "%YAML 1.2\n").unwrap()
        );
    }

    #[test]
    fn yaml_directive_accepts_other_minor_silently() {
        assert_eq!(
            ("", Directive::Yaml { major: 1, minor: 1 }),
            testing::parse(directive, "%YAML 1.1\n").unwrap()
        );
    }

    #[test]
    fn yaml_directive_rejects_major_version_2() {
        testing::parse(directive, "%YAML 2.0\n").unwrap_err();
    }

    #[test]
    fn tag_directive_secondary_handle() {
        assert_eq!(
            (
                "",
                Directive::Tag {
                    handle: "!!",
                    prefix: Cow::Borrowed("tag:example.com,2000:type/")
                }
            ),
            testing::parse(directive, "%TAG !! tag:example.com,2000:type/\n").unwrap()
        );
    }

    #[test]
    fn tag_directive_named_handle() {
        assert_eq!(
            (
                "",
                Directive::Tag {
                    handle: "!e!",
                    prefix: Cow::Borrowed("tag:example.com,2000:")
                }
            ),
            testing::parse(directive, "%TAG !e! tag:example.com,2000:\n").unwrap()
        );
    }

    #[test]
    fn tag_directive_local_prefix() {
        assert_eq!(
            (
                "",
                Directive::Tag {
                    handle: "!",
                    prefix: Cow::Borrowed("!local:")
                }
            ),
            testing::parse(directive, "%TAG ! !local:\n").unwrap()
        );
    }

    /// Spec example 6.13 "Reserved Directives": `%FOO  bar baz # ...` -- name plus two
    /// parameters, trailing comment.
    #[test]
    fn reserved_directive_spec_example_6_13() {
        assert_eq!(
            ("", Directive::Reserved),
            testing::parse(directive, "%FOO  bar baz # Should be ignored\n").unwrap()
        );
    }

    #[test]
    fn reserved_directive_no_parameters() {
        assert_eq!(
            ("", Directive::Reserved),
            testing::parse(directive, "%FOO\n").unwrap()
        );
    }
}
