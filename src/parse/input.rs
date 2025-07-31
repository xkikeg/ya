//! Defines [`InputStream`] which is just a trait alias.

use winnow::{
    stream::{self, Compare, Stream, StreamIsPartial},
    LocatingSlice, Stateful,
};

use super::anchor::AnchorStore;

/// Trait to cover all required traits, which is essentially a trait alias.
pub trait InputStream<'i>:
    Stream<Token = char, Slice = &'i str>
    + StreamIsPartial
    + Compare<&'static str>
    + Compare<char>
    + TrackStartOfLine
    + WithAnchorStore<'i>
    + Clone
{
}

impl<'i, T> InputStream<'i> for T where
    T: Stream<Token = char, Slice = &'i str>
        + StreamIsPartial
        + Compare<&'static str>
        + Compare<char>
        + TrackStartOfLine
        + WithAnchorStore<'i>
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
type InnerInput<'i> = Stateful<LocatingSlice<&'i str>, AnchorStore<'i>>;

impl<'i> Input<'i> {
    /// Creates a new instance.
    pub fn new(from: &'i str) -> Self {
        Self {
            inner: Stateful {
                input: LocatingSlice::new(from),
                state: AnchorStore::new(),
            },
            original: from,
        }
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
        &self.inner.state
    }

    fn anchor_store_mut(&mut self) -> &mut AnchorStore<'i> {
        &mut self.inner.state
    }
}

/// Tracks starting of the line.
pub trait TrackStartOfLine {
    /// Returns `true` when the current position is at the beginning of the line.
    fn is_start_of_line(&self) -> bool;
}

impl<'i> TrackStartOfLine for Input<'i> {
    fn is_start_of_line(&self) -> bool {
        use stream::Location as _;
        match self.inner.previous_token_end() {
            0 => true,
            // Given str is UTF-8, ASCII can be compared literally.
            i => self.original.as_bytes()[i - 1] == b'\n',
        }
    }
}

impl<'i> stream::Offset<<Input<'i> as Stream>::Checkpoint> for Input<'i> {
    fn offset_from(&self, start: &<Input<'i> as Stream>::Checkpoint) -> usize {
        self.inner.offset_from(start)
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
    fn raw(&self) -> &dyn std::fmt::Debug {
        #[allow(deprecated)]
        self.inner.raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_start_of_line() {
        let mut i = Input::new("this\nis\nペン");
        let beginning = i.checkpoint();

        assert_eq!(
            true,
            i.is_start_of_line(),
            "beginning must be also start of line"
        );

        i.next_token();
        assert_eq!(false, i.is_start_of_line(), "middle of line");

        i.next_slice(3);
        assert_eq!(false, i.is_start_of_line(), "right at line break");

        i.next_slice(1);
        assert_eq!(true, i.is_start_of_line(), "beginning of 2nd line");

        i.next_token();
        assert_eq!(false, i.is_start_of_line(), "middle of line");

        i.reset(&beginning);
        assert_eq!(true, i.is_start_of_line(), "back to beginning");

        i.next_slice(8);
        assert_eq!(true, i.is_start_of_line(), "3rd line");

        i.next_token();
        assert_eq!(false, i.is_start_of_line(), "middle of line");
    }
}
