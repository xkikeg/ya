use std::collections::HashMap;

use crate::value::Value;

/// Stores anchor.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AnchorStore<'i>(HashMap<String, Value<'i>>);

impl<'i> AnchorStore<'i> {
    /// Creates a new instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts the given anchor, possibly replacing the previous instance.
    pub fn put(&mut self, key: String, value: Value<'i>) {
        self.0.insert(key, value);
    }

    /// Returns the corresponding anchor if found.
    pub fn get(&self, key: &str) -> Option<&Value<'i>> {
        self.0.get(key)
    }
}
