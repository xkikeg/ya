# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.4.0] - 2026-07-27

Aligns the public API with
[serde's data format conventions](https://serde.rs/conventions.html), and makes parsing lazy at
document granularity.

### Breaking

- `ya::parse` is replaced by `ya::parse_document` (a single document, the common case) and
  `ya::parse_stream` (a lazy iterator over a `---`-separated stream). `parse_stream` returns the
  iterator directly rather than a `Result`: nothing is parsed until it's iterated, so failures
  surface as `Err` items.
- `ya::Deserializer::from_str` returns `Self` instead of `Result<Self>`, for the same reason (and
  matching `serde_json::Deserializer::from_str`). `from_bytes` stays fallible -- the UTF-8 check is
  real.
- `ya::StreamDeserializer` is no longer an `ExactSizeIterator`: a lazy iterator can't know how many
  documents follow.
- `ya::Error` no longer has a lifetime parameter, so it can outlive the parsed input and be
  propagated into `Box<dyn std::error::Error>`, `anyhow::Error`, or any error enum. It is now
  also `Clone + PartialEq` and `#[non_exhaustive]`. `Error::Parse` carries the new
  `ya::OwnedParseError` instead of a boxed winnow `ParseError`.
- `ya::parse` and `ya::from_str` return the new `ya::Result<T>` alias
  (`Result<T, ya::Error>`).

### Added

- `ya::parse_stream` and `ya::Documents`: documents are parsed and tag-resolved one at a time as the
  iterator is advanced, so a multi-document stream costs only its largest single document in peak
  memory and a syntax error in the first document surfaces without parsing the rest.
  `ya::StreamDeserializer` is built on this and is now genuinely lazy. (This is laziness over
  documents, not over input -- the source is still a `&str`.)
- `ya::parse_document`, for the single-document case. An input with no documents (empty, or only
  comments) reads as an implicit null document; one with more than one is `Error::MultipleDocuments`.
- `ya::Error::MultipleDocuments`.
- `parse::yaml_document`, the single-document parser, alongside `parse::yaml_stream` (which is
  unchanged, and still parses a whole stream eagerly).
- `resolve::resolve_document`, resolving one document's tags -- what `resolve` maps over a stream.
- `ya::ParseError<'i>`, a syntax error that keeps the input (and the underlying winnow
  `ParseError`) borrowed for callers who want to inspect more than the rendered message.
  `into_owned()` converts to `OwnedParseError`; both render identically.
- `ya::Result<T>` type alias.
- `ya::Deserializer`, built from the input itself (`Deserializer::from_str` / `from_bytes`), plus
  `ya::StreamDeserializer` via `Deserializer::into_iter::<T>()` for multi-document streams --
  replacing the manual `NodeDeserializer` loop `from_str`'s error used to point at. (Lazy, see
  above.)
- `ya::from_bytes`, deserializing from UTF-8 bytes (`Error::Utf8` on invalid input).
- `impl serde::de::IntoDeserializer for value::Node`, so `node.into_deserializer()` works the way
  `serde_json::Value`'s does.
- `ya::NodeDeserializer` is now re-exported at the crate root alongside `from_str`.
- `value::Stream::into_documents`, handing out owned `Document`s the borrowing `documents()`
  accessor can't.
- `parse::input::Input::original`, returning the complete input regardless of parse position.
- docs.rs now builds with all features, and feature-gated items are labelled as such.

## [0.3.0] - 2026-07-25

Everything below has landed since the `0.2.0` crates.io release.

- Plain (unquoted) scalars, including multi-line folding and the plain/comment disambiguation
  rules.
- Node properties: anchors (`&name`) and all three tag forms (verbatim, shorthand, non-specific),
  with anchor/alias resolution.
- Block scalars: literal (`|`) and folded (`>`), including the chomping matrix and
  auto-detected indentation.
- Block collections: block sequences and block mappings, explicit and implicit entries, and
  compact notation.
- Directives (`%YAML`, `%TAG`, reserved) and explicit/directive documents, including
  multi-document streams.
- Core Schema tag resolution (`resolve()`), plus typed `Node` accessors (`as_bool`, `as_i64`,
  `as_f64`, `as_str`, `is_null`).
- Top-level `ya::parse` convenience function.
- Optional `serde::Deserialize` support behind the `serde` Cargo feature (`ya::from_str`).
- Reached 100% conformance against the official
  [yaml-test-suite](https://github.com/yaml/yaml-test-suite) (`data-2022-01-17`), now checked
  unskipped in CI.
- Added README badges/usage docs, this changelog, and a crate-level doc comment.

See [`AGENT.md`](AGENT.md) for the detailed, phase-by-phase implementation history behind these
entries.

## [0.2.0] - 2025-09-08

Initial published skeleton: the `Node`/`Content` value model split and the input-handling
groundwork the parser is built on top of. Predates plain scalar, block, directive, and tag
resolution support (all listed under Unreleased above).

## [0.1.0] - 2025-07-31

Initial version.
