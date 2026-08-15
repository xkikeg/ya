# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

- A block mapping's `:` now carries the spec's `not followed by non-ws char` lookahead, on both
  `l-block-map-explicit-value` and `c-l-block-map-implicit-value`. Adjacent values (`"a":b`) were
  already rejected, but by the value parser failing downstream rather than by the `:` rule itself.
  ([#52], fixes [#51])

## [0.4.0] - 2026-07-29

Aligns the public API with
[serde's data format conventions](https://serde.rs/conventions.html), makes parsing lazy at
document granularity, and gives every error a source position. Entries are grouped by those three
themes, each linking the pull request it came from.

### Breaking

- **MSRV is now 1.85.0** (was 1.70.0), and `annotate-snippets` joins `winnow` as a second
  dependency. Both follow from rendering diagnostics with `annotate-snippets`, which is edition
  2024. `ya` stays otherwise dependency-free. ([#44])

#### Public API conventions

- `ya::Error` no longer has a lifetime parameter, so it can outlive the parsed input and be
  propagated into `Box<dyn std::error::Error>`, `anyhow::Error`, or any error enum. It is now
  also `Clone + PartialEq` and `#[non_exhaustive]`. `Error::Parse` carries the new
  `ya::OwnedParseError` instead of a boxed winnow `ParseError`. ([#40])
- `ya::from_str`, `ya::parse_document` and the rest of the fallible API return the new
  `ya::Result<T>` alias (`Result<T, ya::Error>`). ([#40])

#### Lazy documents

- `ya::parse` is replaced by `ya::parse_document` (a single document, the common case) and
  `ya::parse_stream` (a lazy iterator over a `---`-separated stream). `parse_stream` returns the
  iterator directly rather than a `Result`: nothing is parsed until it's iterated, so failures
  surface as `Err` items. ([#42])
- `ya::Deserializer::from_str` returns `Self` instead of `Result<Self>`, for the same reason (and
  matching `serde_json::Deserializer::from_str`). `from_bytes` stays fallible -- the UTF-8 check is
  real. ([#42])
- `ya::StreamDeserializer` is no longer an `ExactSizeIterator`: a lazy iterator can't know how many
  documents follow. ([#42])

#### Spans and diagnostics

- `value::Node` carries the `ya::Span` it was parsed from. Its `PartialEq` deliberately **ignores**
  the span -- two nodes are equal when they mean the same thing, regardless of where they were
  written -- so a hand-built node still compares equal to a parsed one. `Node::new`/`unspecified`
  are unchanged (they produce spanless nodes); the span is read with `Node::span()` and set with
  `Node::with_span()`. ([#44])
- `resolve::ResolveError` is a struct rather than an enum: its two former variants moved to
  `resolve::ResolveErrorKind`, behind `ResolveError::kind()`, and it now also carries the span (and,
  once located, the source) of the offending node. ([#44])
- `Error::Custom` (the `serde`-gated variant) is renamed and is now a struct variant,
  `Deserialize { message, excerpt }`. `serde::de::Error::custom` is still what constructs it; the
  variant is named for the phase that failed, like every other one. ([#44], [#46])
- Error `Display` output is now rendered by `annotate-snippets` rather than hand-formatted, so it
  reads like a compiler diagnostic. `OwnedParseError`'s `message`/`offset`/`line`/`column`/
  `line_text` accessors are unchanged. ([#44])

### Added

#### Public API conventions

- `ya::ParseError<'i>`, a syntax error that keeps the input (and the underlying winnow
  `ParseError`) borrowed for callers who want to inspect more than the rendered message.
  `into_owned()` converts to `OwnedParseError`; both render identically. ([#40])
- `ya::Result<T>` type alias. ([#40])
- `ya::Deserializer`, built from the input itself (`Deserializer::from_str` / `from_bytes`), plus
  `ya::StreamDeserializer` via `Deserializer::into_iter::<T>()` for multi-document streams --
  replacing the manual `NodeDeserializer` loop `from_str`'s error used to point at. (Lazy, see
  below.) ([#40])
- `ya::from_bytes`, deserializing from UTF-8 bytes (`Error::Utf8` on invalid input). ([#40])
- `impl serde::de::IntoDeserializer for value::Node`, so `node.into_deserializer()` works the way
  `serde_json::Value`'s does. ([#40])
- `ya::NodeDeserializer` is now re-exported at the crate root alongside `from_str`. ([#40])
- `value::Stream::into_documents`, handing out owned `Document`s the borrowing `documents()`
  accessor can't. ([#37], [#40])
- `parse::input::Input::original`, returning the complete input regardless of parse position.
  ([#40])
- docs.rs now builds with all features, and feature-gated items are labelled as such. ([#40])

#### Lazy documents

- `ya::parse_stream` and `ya::Documents`: documents are parsed and tag-resolved one at a time as the
  iterator is advanced, so a multi-document stream costs only its largest single document in peak
  memory and a syntax error in the first document surfaces without parsing the rest.
  `ya::StreamDeserializer` is built on this and is now genuinely lazy. (This is laziness over
  documents, not over input -- the source is still a `&str`.) ([#42])
- `ya::parse_document`, for the single-document case. An input with no documents (empty, or only
  comments) reads as an implicit null document; one with more than one is
  `Error::MultipleDocuments`. ([#42])
- `ya::Error::MultipleDocuments`. ([#42])
- `parse::yaml_document`, the single-document parser, alongside `parse::yaml_stream` (which is
  unchanged, and still parses a whole stream eagerly). ([#42])
- `resolve::resolve_document`, resolving one document's tags -- what `resolve` maps over a stream.
  ([#42])

#### Spans and diagnostics

- **Source positions on everything.** The parser records the input range of every node it produces
  (`ya::Span`), and errors raised against a node point at the text it was written as -- Core Schema
  tag-resolution failures and, with the `serde` feature, `Deserialize` failures, not just syntax
  errors. Diagnostics are rendered through
  [`annotate-snippets`](https://docs.rs/annotate-snippets) with its plain (uncoloured) renderer,
  since a library caller has no terminal context. ([#44])
- `ya::Span`, the byte range a node was parsed from, with `Node::span()`/`Node::with_span()`.
  ([#44])
- `ya::Excerpt`, the source an error points at: the lines its span covers, plus line/column. On
  `OwnedParseError::excerpt()` and `ResolveError::excerpt()`. ([#44])
- `resolve::ResolveError::span()`, and `located(source)` for callers driving `resolve()` themselves
  (`parse_document`/`parse_stream` call it for you -- they have the input, `resolve()` doesn't).
  ([#44])
- `de::NodeDeserializer::with_source`, so a deserializer built from a bare `Node` can locate its
  errors too. Everything reached through `from_str`/`from_bytes`/`Deserializer` does this already.
  ([#44])
- `Documents::source()`, the complete input being iterated. ([#44])

#### Examples

- `examples/dump.rs`, a CLI over the public API: it dumps each document's `value::Node`, or with
  `--serde` deserializes into a demo type covering every construct `de.rs` supports. Hand-rolled
  argv, so it adds no dependency. ([#45])

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

See [`docs/agents/history.md`](docs/agents/history.md) for the detailed, phase-by-phase
implementation history behind these entries.

## [0.2.0] - 2025-09-08

Initial published skeleton: the `Node`/`Content` value model split and the input-handling
groundwork the parser is built on top of. Predates plain scalar, block, directive, and tag
resolution support (all listed under Unreleased above).

## [0.1.0] - 2025-07-31

Initial version.

[#37]: https://github.com/xkikeg/ya/pull/37
[#40]: https://github.com/xkikeg/ya/pull/40
[#42]: https://github.com/xkikeg/ya/pull/42
[#44]: https://github.com/xkikeg/ya/pull/44
[#45]: https://github.com/xkikeg/ya/pull/45
[#46]: https://github.com/xkikeg/ya/pull/46
[#51]: https://github.com/xkikeg/ya/issues/51
[#52]: https://github.com/xkikeg/ya/pull/52
