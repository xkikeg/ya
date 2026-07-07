use std::borrow::Cow;

use winnow::{
    combinator::{not, opt, trace},
    token::{literal, take_while},
    Parser,
};

use super::{
    chars,
    context::FlowOrKey,
    document,
    error::ParserError,
    input::InputStream,
    spaces::{self, IndentLevel},
};

/// Single quoted text content.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-single-text
#[doc(alias = "nb-single-text")]
pub(super) fn non_break_single_text<'i, Context, Input, Error>(
    _context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "single::non_break_single_text",
        Context::non_break_single_text(indent_level),
    )
}

/// Single quoted text content, multi lines.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-single-multi-line
#[doc(alias = "nb-single-multi-line")]
pub(super) fn non_break_single_multi_line<'i, Input, Error>(
    indent_level: IndentLevel,
) -> impl Parser<Input, Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "single::non_break_single_multi_line",
        move |input: &mut Input| {
            let mut current = non_break_single_one_line.parse_next(input)?;
            loop {
                // See `document::bare_document`'s doc comment for why `c-forbidden` is checked
                // at the fold site rather than once at `l-bare-document`; same hazard as
                // `double::non_break_double_multi_line`'s multi-line fold.
                let may_break = opt((
                    spaces::flow_folded(indent_level),
                    not(document::forbidden).void(),
                ))
                .parse_next(input)?;
                let line_break = match may_break {
                    None => return Ok(current),
                    Some((b, ())) => b,
                };
                // trim the last s-white in the current.
                // Since single-quoted text doesn't have escape,
                // we can literally trim the current string.
                let new_len = current.trim_end_matches(chars::is_white_space).len();
                current.to_mut().truncate(new_len);
                current.to_mut().push_str(&line_break);
                let next_line = non_break_single_one_line.parse_next(input)?;
                current.to_mut().push_str(&next_line);
            }
        },
    )
}

/// Single quoted text content, one line.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-single-one-line
#[doc(alias = "nb-single-one-line")]
pub(super) fn non_break_single_one_line<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("single::non_break_single_one_line", non_break_single_chars).parse_next(input)
}

/// Single quoted chars.
///
/// https://yaml.org/spec/1.2.2/#rule-nb-single-char
#[doc(alias = "nb-single-char")]
fn non_break_single_chars<'i, Input, Error>(
    input: &mut Input,
) -> winnow::Result<Cow<'i, str>, Error>
where
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "single::non_break_single_chars",
        move |input: &mut Input| {
            let mut current: Cow<str> = take_while(0.., is_normal_single_char)
                .map(Cow::Borrowed)
                .parse_next(input)?;
            loop {
                let escaped_quote = opt(literal("''")).parse_next(input)?;
                if escaped_quote.is_none() {
                    return Ok(current);
                }
                current.to_mut().push('\'');
                let next_normal = take_while(0.., is_normal_single_char).parse_next(input)?;
                current.to_mut().push_str(next_normal);
            }
        },
    )
    .parse_next(input)
}

#[inline]
fn is_normal_single_char(c: char) -> bool {
    c != '\'' && chars::is_nb_json(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::{context, testing};

    #[test]
    fn non_break_single_text_single_line() {
        let input = concat!(
            " 1st non-empty\n",
            "\n",
            " 2nd non-empty \n",
            " 3rd non-empty "
        );

        assert_eq!(
            (
                "",
                Cow::Owned(" 1st non-empty\n2nd non-empty 3rd non-empty ".to_string())
            ),
            testing::parse(
                non_break_single_text(context::FlowIn, IndentLevel::initial()),
                input,
            )
            .unwrap()
        );
    }
}
