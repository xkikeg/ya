//! Provides parsers for YAML contents.

pub mod alias;
pub mod anchor;
pub mod block;
pub mod chars;
pub mod context;
mod directive;
// Crate-visible rather than private: `crate::documents`' lazy iterator drives this module's
// `stream_head`/`stream_step` directly, instead of the whole-stream `yaml_stream` parser.
pub(crate) mod document;
mod double;
pub mod error;
pub mod flow;
pub mod input;
pub mod key;
mod plain;
mod properties;
pub mod scalar;
mod single;
pub mod spaces;
mod span;
mod tag_handles;

#[cfg(test)]
pub mod testing;

pub use document::{yaml_document, yaml_stream};
