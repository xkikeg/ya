# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
