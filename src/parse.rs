//! Provides parsers for YAML contents.

pub mod alias;
pub mod anchor;
pub mod block;
pub mod chars;
pub mod context;
mod document;
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
mod tag_handles;

#[cfg(test)]
pub mod testing;

pub use document::yaml_stream;
