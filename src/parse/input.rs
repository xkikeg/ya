//! Defines [`InputStream`] which is just a trait alias.

use winnow::{
    stream::{self, AsBStr, Compare, Location, Stream, StreamIsPartial},
    LocatingSlice, Stateful,
};

use super::anchor::AnchorStore;
use super::tag_handles::TagHandles;

/// Trait to cover all required traits, which is essentially a trait alias.
pub trait InputStream<'i>:
    Stream<Token = char, Slice = &'i str>
    + StreamIsPartial
    + Compare<&'static str>
    + Compare<char>
    + Location
    + TrackStartOfLine
    + WithAnchorStore<'i>
    + WithTagHandles<'i>
    + Clone
{
}

impl<'i, T> InputStream<'i> for T where
    T: Stream<Token = char, Slice = &'i str>
        + StreamIsPartial
        + Compare<&'static str>
        + Compare<char>
        + Location
        + TrackStartOfLine
        + WithAnchorStore<'i>
        + WithTagHandles<'i>
        + Clone
{
}

/// input type to provide extra features in addition to standard input.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Input<'i> {
    inner: InnerInput<'i>,
    original: &'i str,
}

/// internal input type used for [`Input`].
type InnerInput<'i> = Stateful<LocatingSlice<&'i str>, ParseState<'i>>;

/// Parse-time mutable state threaded through the input stream: the document's anchors and the
/// tag-handle -> prefix map.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParseState<'i> {
    anchors: AnchorStore<'i>,
    tag_handles: TagHandles<'i>,
}

impl<'i> Input<'i> {
    /// Creates a new instance.
    pub fn new(from: &'i str) -> Self {
        Self {
            inner: Stateful {
                input: LocatingSlice::new(from),
                state: ParseState::default(),
            },
            original: from,
        }
    }

    /// Returns the complete input this instance was created from, regardless of how far parsing
    /// has advanced. Used by [`crate::ParseError`] to render a diagnostic against the source.
    pub fn original(&self) -> &'i str {
        self.original
    }
}

/// Trait supporting [`AnchorStore`] access, both mutable and immutable.
pub trait WithAnchorStore<'i> {
    /// Get immutable ref.
    fn anchor_store(&self) -> &AnchorStore<'i>;

    /// Get mut ref.
    fn anchor_store_mut(&mut self) -> &mut AnchorStore<'i>;
}

impl<'i> WithAnchorStore<'i> for Input<'i> {
    fn anchor_store(&self) -> &AnchorStore<'i> {
        &self.inner.state.anchors
    }

    fn anchor_store_mut(&mut self) -> &mut AnchorStore<'i> {
        &mut self.inner.state.anchors
    }
}

/// Trait supporting [`TagHandles`] access, both mutable and immutable.
pub trait WithTagHandles<'i> {
    /// Get immutable ref.
    fn tag_handles(&self) -> &TagHandles<'i>;

    /// Get mut ref.
    fn tag_handles_mut(&mut self) -> &mut TagHandles<'i>;
}

impl<'i> WithTagHandles<'i> for Input<'i> {
    fn tag_handles(&self) -> &TagHandles<'i> {
        &self.inner.state.tag_handles
    }

    fn tag_handles_mut(&mut self) -> &mut TagHandles<'i> {
        &mut self.inner.state.tag_handles
    }
}

/// Tracks starting of the line.
pub trait TrackStartOfLine {
    /// Returns `true` when the current position is at the beginning of the line.
    fn is_start_of_line(&self) -> bool;

    /// Returns the character right before the current position, or `None` at the start of input.
    ///
    /// Used to implement the `[lookbehind = ns-char] '#'` alternative of
    /// [`ns-plain-char`](https://yaml.org/spec/1.2.2/#rule-ns-plain-char), since winnow has no
    /// built-in lookbehind support.
    fn previous_char(&self) -> Option<char>;
}

impl<'i> TrackStartOfLine for Input<'i> {
    fn is_start_of_line(&self) -> bool {
        match self.inner.previous_token_end() {
            0 => true,
            // Given str is UTF-8, ASCII can be compared literally.
            i => self.original.as_bytes()[i - 1] == b'\n',
        }
    }

    fn previous_char(&self) -> Option<char> {
        self.original[..self.inner.previous_token_end()]
            .chars()
            .next_back()
    }
}

/// Byte offsets into the input, as [`winnow::Parser::with_span`] needs to hand a
/// [`crate::value::Span`] to each parsed node. Forwarded to the inner [`LocatingSlice`], which is
/// what actually tracks them.
impl<'i> Location for Input<'i> {
    fn previous_token_end(&self) -> usize {
        self.inner.previous_token_end()
    }

    fn current_token_start(&self) -> usize {
        self.inner.current_token_start()
    }
}

impl<'i> stream::Offset<<Input<'i> as Stream>::Checkpoint> for Input<'i> {
    fn offset_from(&self, start: &<Input<'i> as Stream>::Checkpoint) -> usize {
        self.inner.offset_from(start)
    }
}

impl<'i> AsBStr for Input<'i> {
    fn as_bstr(&self) -> &[u8] {
        self.inner.as_bstr()
    }
}

impl<'i> StreamIsPartial for Input<'i> {
    type PartialState = <InnerInput<'i> as StreamIsPartial>::PartialState;

    fn complete(&mut self) -> Self::PartialState {
        self.inner.complete()
    }

    fn restore_partial(&mut self, state: Self::PartialState) {
        self.inner.restore_partial(state);
    }

    fn is_partial_supported() -> bool {
        <InnerInput<'i> as StreamIsPartial>::is_partial_supported()
    }
}

impl<'i, T> Compare<T> for Input<'i>
where
    LocatingSlice<&'i str>: Compare<T>,
{
    fn compare(&self, t: T) -> stream::CompareResult {
        self.inner.compare(t)
    }
}

impl<'i> Stream for Input<'i> {
    type Token = <InnerInput<'i> as Stream>::Token;

    type Slice = <InnerInput<'i> as Stream>::Slice;

    type IterOffsets = <InnerInput<'i> as Stream>::IterOffsets;

    type Checkpoint = <InnerInput<'i> as Stream>::Checkpoint;

    #[inline(always)]
    fn iter_offsets(&self) -> Self::IterOffsets {
        self.inner.iter_offsets()
    }

    #[inline(always)]
    fn eof_offset(&self) -> usize {
        self.inner.eof_offset()
    }

    #[inline(always)]
    fn next_token(&mut self) -> Option<Self::Token> {
        self.inner.next_token()
    }

    #[inline(always)]
    fn peek_token(&self) -> Option<Self::Token> {
        self.inner.peek_token()
    }

    #[inline(always)]
    fn offset_for<P>(&self, predicate: P) -> Option<usize>
    where
        P: Fn(Self::Token) -> bool,
    {
        self.inner.offset_for(predicate)
    }

    #[inline(always)]
    fn offset_at(&self, tokens: usize) -> Result<usize, winnow::error::Needed> {
        self.inner.offset_at(tokens)
    }

    #[inline(always)]
    fn next_slice(&mut self, offset: usize) -> Self::Slice {
        self.inner.next_slice(offset)
    }

    #[inline(always)]
    unsafe fn next_slice_unchecked(&mut self, offset: usize) -> Self::Slice {
        // SAFETY: Passing up invariants
        unsafe { self.inner.next_slice_unchecked(offset) }
    }

    #[inline(always)]
    fn peek_slice(&self, offset: usize) -> Self::Slice {
        self.inner.peek_slice(offset)
    }

    #[inline(always)]
    unsafe fn peek_slice_unchecked(&self, offset: usize) -> Self::Slice {
        // SAFETY: Passing up invariants
        unsafe { self.inner.peek_slice_unchecked(offset) }
    }

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        self.inner.checkpoint()
    }

    #[inline(always)]
    fn reset(&mut self, checkpoint: &Self::Checkpoint) {
        self.inner.reset(checkpoint)
    }

    #[inline(always)]
    fn trace(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.trace(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_start_of_line() {
        let mut i = Input::new("this\nis\nペン");
        let beginning = i.checkpoint();

        assert!(i.is_start_of_line(), "beginning must be also start of line");

        i.next_token();
        assert!(!i.is_start_of_line(), "middle of line");

        i.next_slice(3);
        assert!(!i.is_start_of_line(), "right at line break");

        i.next_slice(1);
        assert!(i.is_start_of_line(), "beginning of 2nd line");

        i.next_token();
        assert!(!i.is_start_of_line(), "middle of line");

        i.reset(&beginning);
        assert!(i.is_start_of_line(), "back to beginning");

        i.next_slice(8);
        assert!(i.is_start_of_line(), "3rd line");

        i.next_token();
        assert!(!i.is_start_of_line(), "middle of line");
    }
}
