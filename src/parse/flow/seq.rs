use winnow::{
    combinator::{alt, delimited, opt, trace},
    token::one_of,
    Parser,
};

use crate::{
    parse::{
        context::{FlowOrKey, InFlow, YamlContext},
        error::ParserError,
        input::InputStream,
        spaces::{self, IndentLevel},
        span::spanned,
    },
    value::{Content, Mapping, Node},
};

/// Flow sequence.
///
/// https://yaml.org/spec/1.2.2/#rule-c-flow-sequence
#[doc(alias = "c-flow-sequence")]
pub fn flow_sequence<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Vec<Node<'i>>, Error>
where
    Context: FlowOrKey,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::seq::flow_sequence",
        delimited(
            (one_of('['), opt(spaces::separate(context, indent_level))),
            // `ns-s-flow-seq-entries(n,c)` itself always matches >= 1 entry; per
            // `c-flow-sequence`'s own composition, it's the trailing `?` *here*, at the call
            // site, that allows a completely empty `[]`.
            opt(flow_seq_entries(
                <Context::Flow as YamlContext>::get(),
                indent_level,
            ))
            .map(Option::unwrap_or_default),
            one_of(']'),
        ),
    )
}

/// Flow sequence entry list.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-s-flow-seq-entries
#[doc(alias = "ns-s-flow-seq-entries")]
pub fn flow_seq_entries<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Vec<Node<'i>>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace("flow::seq::flow_seq_entries", move |input: &mut Input| {
        let entry = flow_seq_entry(context, indent_level).parse_next(input)?;
        let mut ret = vec![entry];
        loop {
            let comma = delimited(
                opt(spaces::separate(context, indent_level)),
                opt(one_of(b',')),
                opt(spaces::separate(context, indent_level)),
            )
            .parse_next(input)?;
            if comma.is_none() {
                return Ok(ret);
            }
            let next = opt(flow_seq_entry(context, indent_level)).parse_next(input)?;
            match next {
                None => return Ok(ret),
                Some(next) => ret.push(next),
            }
        }
    })
}

/// Flow sequence single entry.
///
/// https://yaml.org/spec/1.2.2/#rule-ns-flow-seq-entry
#[doc(alias = "ns-flow-seq-entry")]
pub fn flow_seq_entry<'i, Context, Input, Error>(
    context: Context,
    indent_level: IndentLevel,
) -> impl Parser<Input, Node<'i>, Error>
where
    Context: InFlow,
    Input: InputStream<'i>,
    Error: ParserError<Input>,
{
    trace(
        "flow::seq::flow_seq_entry",
        alt((
            move |input: &mut Input| {
                spanned(
                    input,
                    super::pair::flow_pair(context, indent_level)
                        .map(|e| Node::unspecified(Content::Map(Mapping(vec![e])))),
                )
            },
            super::node::flow_node(context, indent_level),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::{context::FlowOut, testing};

    /// `ns-s-flow-seq-entries(n,c)` itself always matches >= 1 entry; the empty-sequence case
    /// (corpus case `7ZZ5` "Empty flow collections", among others) depends on `c-flow-sequence`
    /// making that whole group optional at its own call site instead.
    #[test]
    fn empty_sequence() {
        let (rest, got) =
            testing::parse(flow_sequence(FlowOut, IndentLevel::initial()), "[]").unwrap();
        assert_eq!("", rest);
        assert_eq!(Vec::<Node<'_>>::new(), got);
    }

    #[test]
    fn empty_sequence_with_inner_separation() {
        let (rest, got) =
            testing::parse(flow_sequence(FlowOut, IndentLevel::initial()), "[   ]").unwrap();
        assert_eq!("", rest);
        assert_eq!(Vec::<Node<'_>>::new(), got);
    }
}
