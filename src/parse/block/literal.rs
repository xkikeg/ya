use std::borrow::Cow;

use winnow::{
    combinator::{opt, preceded, repeat, trace},
    token::take_while,
    Parser,
};

use super::header::{self, ChompingMode};
use crate::parse::{
    chars,
    context::BlockIn,
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// Literal block scalar.
///
/// https://yaml.org/spec/1.2.2/#rule-c-l+literal
#[doc(alias = "c-l+literal")]
pub(super) fn literal<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::literal::literal", move |input: &mut Input| {
        '|'.parse_next(input)?;
        let (indentation, chomping) = header::block_header.parse_next(input)?;
        let content_indent = match indentation {
            Some(m) => indent_level + m,
            None => header::detect_indentation.parse_next(input)?,
        };
        literal_content(content_indent, chomping).parse_next(input)
    })
}

/// A single literal-style content line: any leading empty lines, followed by one indented,
/// non-empty line of raw (unescaped) content.
///
/// https://yaml.org/spec/1.2.2/#rule-l-nb-literal-text
#[doc(alias = "l-nb-literal-text")]
fn literal_text<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::literal::literal_text", move |input: &mut Input| {
        let count: usize =
            repeat(0.., spaces::line_empty(BlockIn, indent_level)).parse_next(input)?;
        let text: &str = preceded(
            spaces::indent(indent_level),
            take_while(1.., chars::is_non_break),
        )
        .parse_next(input)?;
        if count > 0 {
            let mut s = "\n".repeat(count);
            s.push_str(text);
            Ok(Cow::Owned(s))
        } else {
            Ok(Cow::Borrowed(text))
        }
    })
}

/// A line break followed by another [`literal_text`] line.
///
/// https://yaml.org/spec/1.2.2/#rule-b-nb-literal-next
#[doc(alias = "b-nb-literal-next")]
fn literal_next<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::literal::literal_next", move |input: &mut Input| {
        chars::break_as_line_feed.parse_next(input)?;
        let text = literal_text(indent_level).parse_next(input)?;
        let mut s = String::with_capacity(1 + text.len());
        s.push('\n');
        s.push_str(&text);
        Ok(Cow::Owned(s))
    })
}

/// Full literal scalar content: the text lines (if any), chomped according to the header's
/// chomping mode.
///
/// https://yaml.org/spec/1.2.2/#rule-l-literal-content
#[doc(alias = "l-literal-content")]
fn literal_content<'i, Input, Error>(
    indent_level: IndentLevel,
    chomping: ChompingMode,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("block::literal::literal_content", move |input: &mut Input| {
        let mut current: Cow<str> = match opt(literal_text(indent_level)).parse_next(input)? {
            Some(mut text) => {
                loop {
                    match opt(literal_next(indent_level)).parse_next(input)? {
                        Some(next) => text.to_mut().push_str(&next),
                        None => break,
                    }
                }
                let last = header::chomped_last(chomping).parse_next(input)?;
                if !last.is_empty() {
                    text.to_mut().push_str(last);
                }
                text
            }
            None => Cow::Borrowed(""),
        };
        let trailing = header::chomped_empty(indent_level, chomping).parse_next(input)?;
        if !trailing.is_empty() {
            current.to_mut().push_str(&trailing);
        }
        Ok(current)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::testing;

    /// Spec example 8.5 "Chomping Final Line Break", literal half only.
    #[test]
    fn literal_clip_keeps_final_break() {
        assert_eq!(
            ("", Cow::Borrowed("text\n")),
            testing::parse(literal(IndentLevel::new(0)), "|\n  text\n").unwrap()
        );
    }

    #[test]
    fn literal_strip_drops_final_break() {
        assert_eq!(
            ("", Cow::Borrowed("text")),
            testing::parse(literal(IndentLevel::new(0)), "|-\n  text\n").unwrap()
        );
    }

    #[test]
    fn literal_keep_keeps_trailing_empty_lines() {
        let (rest, got) =
            testing::parse(literal(IndentLevel::new(0)), "|+\n  text\n").unwrap();
        assert_eq!("", rest);
        assert_eq!(Cow::<str>::Owned("text\n".to_string()), got);
    }

    /// Spec example 8.7 "Literal Scalar".
    #[test]
    fn literal_scalar_example() {
        assert_eq!(
            ("", Cow::Owned("literal\n\ttext\n".to_string())),
            testing::parse(literal(IndentLevel::initial()), "|\n literal\n \ttext\n\n").unwrap()
        );
    }

    /// Spec example 8.9 "Literal Content".
    #[test]
    fn literal_content_example() {
        let input = "|\n \n  \n  literal\n   \n  \n  text\n\n # Comment\n";
        assert_eq!(
            ("", Cow::Owned("\n\nliteral\n \n\ntext\n".to_string())),
            testing::parse(literal(IndentLevel::initial()), input).unwrap()
        );
    }
}
