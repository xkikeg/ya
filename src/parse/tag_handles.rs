use std::borrow::Cow;
use std::collections::HashMap;

/// Tag handle (`!`, `!!`, or a named `!foo!` handle) -> prefix map.
///
/// The two default handles ([`c-primary-tag-handle`](https://yaml.org/spec/1.2.2/#rule-c-primary-tag-handle)
/// and [`c-secondary-tag-handle`](https://yaml.org/spec/1.2.2/#rule-c-secondary-tag-handle)) are
/// always present; named handles only exist once a `%TAG` directive registers one, so looking one
/// up before its directive has been parsed always misses.
#[derive(Debug, Clone, PartialEq)]
pub struct TagHandles<'i>(HashMap<Cow<'i, str>, Cow<'i, str>>);

impl<'i> TagHandles<'i> {
    /// Creates a new instance with just the two default handles.
    pub fn new() -> Self {
        let mut map = HashMap::new();
        map.insert(Cow::Borrowed("!"), Cow::Borrowed("!"));
        map.insert(Cow::Borrowed("!!"), Cow::Borrowed("tag:yaml.org,2002:"));
        Self(map)
    }

    /// Returns the prefix associated with the given handle, if declared.
    pub fn get(&self, handle: &str) -> Option<&Cow<'i, str>> {
        self.0.get(handle)
    }

    /// Registers a handle -> prefix mapping, replacing any previous one. Used by `%TAG`
    /// directives.
    pub fn put(&mut self, handle: Cow<'i, str>, prefix: Cow<'i, str>) {
        self.0.insert(handle, prefix);
    }

    /// Resets to just the two default handles, e.g. at a document boundary: `%TAG` directives are
    /// document-scoped, so a named handle registered in one document must not leak into the next.
    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

impl<'i> Default for TagHandles<'i> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_handles_are_preregistered() {
        let handles = TagHandles::new();
        assert_eq!(Some(&Cow::Borrowed("!")), handles.get("!"));
        assert_eq!(
            Some(&Cow::Borrowed("tag:yaml.org,2002:")),
            handles.get("!!")
        );
        assert_eq!(None, handles.get("!e!"));
    }

    #[test]
    fn put_registers_named_handle() {
        let mut handles = TagHandles::new();
        handles.put(
            Cow::Borrowed("!e!"),
            Cow::Borrowed("tag:example.com,2000:app/"),
        );
        assert_eq!(
            Some(&Cow::Borrowed("tag:example.com,2000:app/")),
            handles.get("!e!")
        );
    }
}
