# AGENT.md

Guidance for agents (and humans) working on `ya`, a pure-Rust YAML 1.2.2 parser.

## What this project is

`ya` ("yet another YAML parser") implements the [YAML 1.2.2 specification](https://yaml.org/spec/1.2.2/)
using the [`winnow`](https://docs.rs/winnow) parser-combinator crate. The defining design goal is a
**literal, 1:1 mapping between the spec grammar and the code**: every production rule in the spec
(`ns-plain`, `c-flow-mapping`, `s-l+block-node`, ...) has a corresponding Rust function, named after
the rule, documented with a link to its exact anchor in the spec, and tagged with
`#[doc(alias = "rule-name")]` so it's discoverable both by spec name and Rust name. When in doubt about
*what* a function should do or *what its name means*, open the spec section it links to — that section is
the authoritative source, not prior Rust idioms or convenience.

Consequences of this design for how you should work here:

- **Don't "clean up" toward idiomatic-but-non-spec-shaped code.** If the grammar defines a rule as a
  composition of three sub-rules, prefer three composed parser functions over one fused/optimized one,
  even if the fused version is shorter. The spec correspondence is the point.
- **Preserve naming.** Function names transliterate the spec's kebab-case rule names to snake_case
  (`ns-plain-first` → `non_space_plain_first`, `l+block-mapping` → `block_mapping`). Keep this convention
  for anything new. Keep the `#[doc(alias = "...")]` and the `/// https://yaml.org/spec/1.2.2/#rule-...`
  link on every new parser function.
- **Match the spec's parameterization.** The spec defines productions parameterized by *indentation
  level* `n` and *context* `c` (`block-out`, `block-in`, `flow-out`, `flow-in`, `block-key`, `flow-key`).
  This codebase mirrors that exactly via `IndentLevel` (see `src/parse/spaces.rs`) and the `YamlContext`
  trait family (see `src/parse/context.rs`) rather than passing raw enums/booleans around. Extend those
  traits when a new context-dependent rule is needed; don't bypass them with ad hoc parameters.
- Since production correctness is checked against the spec text, prefer adding a unit test with the
  exact example strings from the spec (many spec sections include one) over inventing new fixtures.

## Architecture

```
src/
  lib.rs             re-exports `parse` and `value`
  value.rs            output data model (Stream, Document, Node, Content, Scalar, Mapping, Tag)
  parse.rs            module root, re-exports `yaml_stream` as the public entry point
  parse/
    error.rs          ParserError trait alias (winnow error bounds used throughout)
    input.rs           InputStream trait alias + `Input` (LocatingSlice + Stateful<AnchorStore>)
                        + `WithLimit` (bounds input length, used for the 1024-char implicit-key limit)
    context.rs         YamlContext / NonKey / InOutFlow / InOutBlock / Key / InFlow / FlowOrKey traits;
                        BlockIn / BlockOut / BlockKey / FlowIn / FlowOut / FlowKey marker types
                        => this is the spec's context parameter `c`, reified as types
    spaces.rs           Chapter 6 structural productions: indentation, s-separate, line folding,
                        comments, IndentLevel type (spec's indentation parameter `n`)
    chars.rs            Chapter 5 character-class predicates and line-break parsing
    anchor.rs           AnchorStore (parse-time symbol table for anchors, keyed by name)
    alias.rs            c-ns-alias-node (`*name`), resolved against AnchorStore
    key.rs              c-s-implicit-yaml-key / c-s-implicit-json-key (1024-char-limited keys used
                        as flow mapping/pair keys)
    single.rs           single-quoted flow scalar content (nb-single-*)
    double.rs           double-quoted flow scalar content incl. escape sequences (nb-double-*)
    plain.rs            plain (unquoted) flow scalar content (ns-plain-*) -- WEAKEST part of the tree
    scalar.rs           ties single/double quoted content to c-single-quoted / c-double-quoted
    document.rs         Chapter 9: l-yaml-stream, l-any-document, l-bare-document, document
                        prefix/suffix; public `yaml_stream()` entry point + its tests
    flow/
      content.rs         ns-flow-content / ns-flow-yaml-content / ns-flow-json-content
      node.rs             ns-flow-node / ns-flow-yaml-node / ns-flow-json-node
      seq.rs              c-flow-sequence and its entries
      map.rs              c-flow-mapping and its entries (explicit/implicit/yaml-key/json-key)
      pair.rs             ns-flow-pair (used for flow-sequence "single pair" shorthand entries)
    block/
      header.rs           c-chomping-indicator only so far (block header is otherwise unimplemented)
      scalar.rs           s-l+block-scalar (STUB, see below)
      seq.rs               l+block-sequence / c-l-block-seq-entry
      map.rs               l+block-mapping (STUB block_map_entry, see below)
      node.rs             s-l+block-node / s-l+block-in-block (STUB) / s-l+flow-in-block (done) /
                          s-l+block-indented (STUB)
    testing.rs (test-only) parse-helper wrapping winnow's `.parse()` for use in unit tests
```

## Implementation status

Roughly implemented and unit-tested:
- Chapter 5 character classes (`chars.rs`).
- Chapter 6 spacing/indentation/comments/line-folding (`spaces.rs`) -- good coverage, one internal
  TODO to switch an `alt` to `dispatch!` for clarity/perf (`spaces.rs:154`), not a correctness gap.
- Flow scalars: single-quoted and double-quoted, including escapes and multi-line folding
  (`single.rs`, `double.rs`).
- Flow collections: sequences, mappings (explicit/implicit/yaml-key/json-key entries), pairs
  (`flow/seq.rs`, `flow/map.rs`, `flow/pair.rs`).
- Aliases and the anchor store (`alias.rs`, `anchor.rs`) -- but nothing populates the anchor store yet
  because anchor *properties* aren't parsed (see gaps).
- Document stream skeleton for the *bare document* case (`document.rs::yaml_stream`,
  `bare_document`), with two working unit tests (flow seq, flow map).

Not implemented (stub returns `fail`, or missing outright) -- these are exactly the blockers a
`cargo build` warning-free pass would still leave semantically incomplete:
- `plain.rs::non_space_plain_chars` -- stub. `plain.rs::non_space_plain_multi_line` -- stub.
  `non_space_plain_one_line` only ever consumes a single first character today (it does not call
  `non_space_plain_chars` in a loop), so **plain scalars are effectively unusable** beyond one char.
  This is the single most impactful gap since plain scalars are YAML's most common scalar style.
- Node properties (anchor `&name` / tag `!!str`, `!<...>`, `!handle!suffix`) are entirely unparsed.
  `flow/node.rs` has three `// TODO: fixme Support properties.` markers; there is no `properties.rs`
  module and no tag-handle resolution yet. `value::Tag` exists as a data type but nothing produces
  non-`Unspecified` tags.
- Directives (`%YAML`, `%TAG`, reserved directives) -- `document.rs::directive_document` is `fail`.
- Explicit documents (`--- ... ...`) -- `document.rs::explicit_document` is `fail`.
- Block mapping entries -- `block/map.rs::block_map_entry` is `fail`, so `block_mapping` cannot
  actually produce a mapping yet even though its outer loop/indent logic is written.
- Block scalar content (literal `|` and folded `>`) -- `block/scalar.rs::block_scalar` is `fail`;
  `block/header.rs` only has the chomping indicator, not the indentation indicator or the header
  parse as a whole.
- `block/node.rs::block_in_block` (dispatches to block sequence/mapping/scalar by lookahead) and
  `block_indented` (content of a `-` sequence entry, including compact notation) are both `fail`.
  `flow_in_block` (the flow-node-inside-a-block-context case) *is* implemented.
- Tag resolution / Core Schema (turning an unspecified-tag plain scalar into `Null`/`Bool`/`Int`/
  `Float`/string per the core schema) is deferred by design (see comment in `plain.rs::plain`) but
  nothing implements that later stage yet either.
- `tests/integration_tests.rs` walks the `yaml-test-suite` submodule and parses every `in.yaml`, but
  the success path never actually compares against `test.event` (`// TODO: compare stream against the
  event.`), so passing that test currently only proves "didn't error", not "parsed correctly".

## Conventions worth knowing before touching code

- Parsers are generic over `Input: InputStream<'i>` and `Error: ParserError<Input>`, and (when
  context-dependent) a `Context: <SomeContextTrait>` type parameter, returning `impl Parser<Input, T,
  Error>`. Follow this signature shape for new rule functions rather than hand-writing
  `fn(&mut Input) -> Result<...>` unless the rule is genuinely context-free (see `chars.rs`,
  `spaces.rs::separate_in_line` for examples of the free-function form winnow also supports).
- Every parser is wrapped in `winnow::combinator::trace("module::function_name", ...)` using a
  dotted path matching its location. Keep this when adding parsers; it's the tracing/debugging story
  for this crate (enable with winnow's `debug` feature).
- Scalar text is `Cow<'i, str>`: borrow from the input when no unescaping/folding is needed, allocate
  only when necessary (escapes, line folding). Preserve this discipline in new scalar-producing code
  -- don't reflexively `.to_string()`.
- Unimplemented rules are stubbed as `trace("path::name", winnow::combinator::fail)` with a `// TODO`
  comment, not `todo!()`/`unimplemented!()`. This lets `alt((...))` combinators compile and backtrack
  cleanly today even where a branch isn't implemented yet. Follow this pattern for any new
  not-yet-implemented rule rather than panicking.
- `IndentLevel` internally stores spec `n + 1` (so the spec's `n = -1` "no indent yet" becomes `0`);
  use `IndentLevel::initial()` / `IndentLevel::new(n)` / `.get()` rather than constructing the
  wrapped integer directly.
- The compiled crate depends on `winnow` 0.7.x; when bumping it, check for combinator signature
  changes first (the compile error fixed alongside this doc, `trace()` gaining a second required
  argument in a newer winnow release, was exactly this kind of break).

## Testing

- `cargo test` runs unit tests colocated in each module (`#[cfg(test)] mod tests`) plus
  `tests/integration_tests.rs`.
- `testdata/yaml-test-suite` is a git submodule (the official
  [yaml/yaml-test-suite](https://github.com/yaml/yaml-test-suite)); it must be checked out
  (`git submodule update --init`) for the integration test to find cases. It is pinned to tag
  `data-2022-01-17`.
- The integration test both checks "error cases fail to parse" and, for valid cases, structurally
  compares the parse against the suite's `test.event` fixture (representation-level: node shape,
  scalar content, resolved tag -- not presentation style or anchor names; see Phase 7 below for why).
  Run `cargo test --test integration_tests conformance_report -- --nocapture` (or just read
  `target/yaml_conformance_report.txt` after any `cargo test`) for the current pass-rate breakdown.
- `benches/benchmark.rs` (Criterion) benchmarks `flow_sequence` only today; its own comment notes
  plain scalars aren't benchmarked yet because they're unsupported.

## TODO: path to a complete parser

Ordered so each phase mostly only depends on earlier ones. Pick items off the top; check off /
update this list as work lands so it stays a reliable map of what's left. Each item names the spec
rule(s) it corresponds to and the file(s) most directly involved.

### Phase 1 -- Plain scalars (highest priority: most common YAML scalar style, currently broken)
- [ ] `ns-plain-char` (`plain.rs::non_space_plain_chars`): implement per spec --
      `ns-plain-safe(c)` minus `:`/`#`, or `ns-char` immediately followed by `#`, or `:` immediately
      followed by `ns-plain-safe(c)`.
- [ ] `ns-plain-one-line` (`plain.rs::non_space_plain_one_line`): change from "take first char only"
      to `ns-plain-first(c) nb-ns-plain-in-line(c)*`, i.e. loop `(s-white* ns-plain-char(c))*` after
      the first char, using the fix above.
- [ ] `ns-plain-multi-line` / `nb-ns-plain-in-line` / `s-ns-plain-next-line` / `s-ns-plain-first`
      (`plain.rs::non_space_plain_multi_line`): implement multi-line plain scalar folding, mirroring
      the fold/trim logic already present in `single.rs::non_break_single_multi_line` and
      `double.rs::non_break_double_multi_line` (same shape, no escapes to worry about, but leading
      empty-line handling differs slightly per spec).
- [ ] Add unit tests using the plain-scalar examples from
      [spec §7.3.3](https://yaml.org/spec/1.2.2/#733-plain-style).

### Phase 2 -- Node properties (anchors & tags)
- [ ] New `parse/properties.rs`: `c-ns-properties` (tag + anchor, either order, either optional but
      not both absent... actually spec allows either alone), `c-ns-tag-property`, `c-verbatim-tag`,
      `c-ns-shorthand-tag`, `c-non-specific-tag`, `c-ns-anchor-property`, `ns-anchor-name`,
      `ns-anchor-char`.
- [ ] Tag handles: `c-tag-handle` (`c-named-tag-handle` / `c-secondary-tag-handle` /
      `c-primary-tag-handle`), `ns-tag-char`; needs a per-document handle→prefix map (populated by
      `%TAG` directives in Phase 5, defaulted per spec §6.9.1 otherwise) threaded through parsing --
      likely lives on `AnchorStore`'s sibling state or a new `TagStore` in `input.rs`'s `Stateful`.
- [ ] Wire anchor parsing into `alias.rs`/`anchor.rs`: when a node has an anchor property, register
      it in `AnchorStore` via `input.anchor_store_mut().put(...)` *as the node finishes parsing*
      (need the fully-built `Node`, so this likely wraps `flow_node`/`block_node`, not `flow_content`).
- [ ] Update `flow/node.rs` (`flow_node`, `flow_yaml_node`, `flow_json_node`) and `block/node.rs`
      (`block_node`) to parse properties and attach them to the returned `Node` (replace the three
      `// TODO: fixme Support properties.` markers).
- [ ] Decide & document how `value::Tag::Global` vs `Tag::Standard` gets picked at this stage vs.
      resolution stage (Phase 6) -- likely: parsing captures the raw resolved-handle tag URI,
      Phase 6 maps well-known URIs (`tag:yaml.org,2002:str` etc.) to `Tag::Standard`.

### Phase 3 -- Block scalars (literal `|` / folded `>`)
- [ ] `c-b-block-header` and `c-indentation-indicator` (`block/header.rs`): currently only chomping
      exists; add the indentation indicator (explicit digit, or auto-detected from first non-empty
      line per §8.1.1.2) and combine into one `block_header` parser returning
      `(IndentLevel, ChompingMode)`.
- [ ] `c-l+literal` / `l-literal-content` (new, e.g. `block/literal.rs`).
- [ ] `c-l+folded` / `l-folded-content` / `s-nb-folded-text` / `l-nb-folded-lines` / `s-flow-folded`
      reuse where possible (new, e.g. `block/folded.rs`).
- [ ] Implement `block/scalar.rs::block_scalar` to dispatch on `|` vs `>` and delegate; wire the
      header's indentation indicator into child parsers' `IndentLevel`.
- [ ] Unit tests from [spec §8.1](https://yaml.org/spec/1.2.2/#81-block-scalar-styles) (there are
      several worked chomping/indentation examples there worth transcribing directly).

### Phase 4 -- Block collections
- [ ] `ns-l-block-map-entry` / `c-l-block-map-explicit-entry` / `ns-l-block-map-implicit-entry` /
      `ns-s-block-map-implicit-key` / `c-l-block-map-implicit-value` / `ns-block-map-entry`
      (`block/map.rs::block_map_entry`): implement explicit (`?`) and implicit (`key:`) entries,
      including the "compact" same-line nested collection case.
      `block_mapping`'s outer indent/repeat loop is already correct; only the entry itself is missing.
- [ ] `s-l+block-indented` / `ns-l-compact-sequence` / `ns-l-compact-mapping`
      (`block/node.rs::block_indented`): content of a `-` sequence entry -- either a nested
      `s-l+block-node`, a same-line compact seq/map, or empty.
- [ ] `s-l+block-in-block` / `s-l+block-collection` / `seq-space`
      (`block/node.rs::block_in_block`): dispatch by lookahead to block sequence, block mapping
      (regular or "in-sequence, no leading spaces" per §8.2.2), or block scalar (Phase 3), applying
      node properties from Phase 2 first.
- [ ] Once entries work, revisit `block/seq.rs` for the compact-in-mapping edge case
      (`l+block-sequence` allows `n-1` indentation when nested directly under a mapping value).

### Phase 5 -- Directives & explicit/full documents
- [ ] New `parse/directive.rs`: `l-directive`, `ns-yaml-directive`, `ns-yaml-version`,
      `ns-tag-directive`, `ns-tag-handle`, `ns-tag-prefix`, `ns-reserved-directive`,
      `ns-directive-name`, `ns-directive-parameter`. `%YAML` directive should validate/track version
      (warn or error on unsupported major version per spec); `%TAG` should populate the handle→prefix
      map from Phase 2.
- [ ] `document.rs::directive_document`: `l-directive-document` = one or more directives, then
      `l-explicit-document`.
- [ ] `document.rs::explicit_document`: `l-explicit-document` = `"---"` marker, then optional
      `l-bare-document`, i.e. an explicit document may be empty (`Content::Empty` node).
- [ ] Re-check `document.rs::yaml_stream`/`any_document` once the above land -- the outer stream loop
      already anticipates directive/explicit documents via `alt`, it just can't reach them yet.
- [ ] Reset per-document parse state (tag handle map at least; anchors are also meant to be
      document-scoped per spec §3.2.2.2) at each document boundary.

### Phase 6 -- Tag resolution / Core Schema
- [ ] Implement the [Core Schema](https://yaml.org/spec/1.2.2/#103-core-schema) resolution: given an
      unspecified-tag scalar's content and style (plain vs quoted -- quoted scalars are never
      resolved beyond `str`), decide `Null`/`Bool`/`Int`/`Float`/`Str` and produce the matching
      `value::Scalar` variant, replacing today's blanket `Scalar::SingleStr`/`DoubleStr` for plains.
      (`plain.rs::plain`'s doc comment already flags this as an intentionally deferred later stage.)
  - [ ] `tag:yaml.org,2002:null` regexes/literals (`~`, `null`, `Null`, `NULL`, empty).
  - [ ] `tag:yaml.org,2002:bool` (`true`/`false` and case variants per core schema).
  - [ ] `tag:yaml.org,2002:int` (decimal/octal/hex forms).
  - [ ] `tag:yaml.org,2002:float` (incl. `.inf`/`.nan` variants).
  - [ ] Map explicit `!!null`/`!!bool`/`!!int`/`!!float`/`!!str`/`!!map`/`!!seq` tags (once Phase 2
        parses them) to `value::StandardTag` / forced scalar reinterpretation, erroring if the
        content doesn't match the forced tag.
- [ ] Decide where resolution runs: as a post-pass over `value::Node`, or inline during parse. A
      post-pass is probably simpler and keeps parsing itself schema-agnostic (consistent with the
      "schema applied at later stage" comment already in the code).

### Phase 7 -- Conformance harness
- [x] `tests/integration_tests.rs::check_input`: actually parse each case's `test.event` (libyaml
      event-stream format) and compare against the parsed `value::Stream`, instead of only checking
      "parses without error". Implemented via a shared `ExpectedNode` tree type: the event-format
      text is parsed into that tree (with `=ALI` aliases expanded against a locally-built anchor map
      while walking, mirroring `alias.rs`'s own eager substitution), `ya`'s own `value::Stream` is
      converted into the same tree shape, and a recursive `diff_nodes` reports the first point of
      divergence, categorized as `StructuralMismatch` (wrong node kind / seq-map length -- usually an
      unimplemented-grammar stub) vs. `ContentMismatch` (right shape, wrong scalar value/tag).
      Required two small new public accessors, `value::Stream::documents()` and
      `value::Mapping::entries()` (their backing `Vec`s were `pub(crate)`, which is exactly why this
      TODO couldn't be done from `tests/` -- an external crate -- before).
      **Deliberately out of scope**: presentation style (plain vs. single-quoted, flow vs. block) and
      anchor *names* are not compared, because `value::Node` can't represent either yet (and
      `Scalar::SingleStr` currently covers both plain and single-quoted, see Phase 1) -- comparison
      focuses on the representation graph (shape + content + resolved tag), matching this crate's
      stated end goal of serde/Construct-phase deserialization over presentation round-tripping.
- [x] Track/report pass rate: `tests/integration_tests.rs::conformance_report` (a plain, non-ignored,
      never-failing `#[test]`) walks the whole corpus, tallies pass/fail by category (adding
      `UnexpectedSuccessOnErrorCase` for error-cases the parser wrongly accepts, `MalformedFixture`
      for unreadable/unparseable fixtures, and `ParserPanic` for cases where `ya` itself panics
      instead of returning a parse error -- caught via `catch_unwind` with a replaced panic hook so
      one crashing input can't lose the whole report), and writes a full breakdown to both stdout and
      `target/yaml_conformance_report.txt`. Run with `cargo test --test integration_tests
      conformance_report -- --nocapture` to see it inline, or just `cat` the file after any
      `cargo test`. As of this writing: **~114/402 (28%)** passing -- expected, given how much of
      Phases 1-6 is still unimplemented; re-run to get a current number as later phases land.
      Two real bugs the harness surfaced while implementing this (not fixed here, still open):
      `ya` panics (a tripped `winnow` "`repeat` parsers must always consume" invariant) on
      completely empty input and on input consisting solely of a `...` document-end marker
      (corpus cases `AVM7`, `HWV9`); and double-quoted scalar parsing drops a tab character
      adjacent to an escaped space (corpus cases `02`/`03`, "Trailing tabs in double quoted").
- [ ] Once Phase 1-6 land, revisit `benches/benchmark.rs`'s commented-out plain-scalar lines.

### Phase 8 -- Polish (do last, or opportunistically)
- [ ] Clear the existing `cargo build` warnings (unused imports/params in `scalar.rs`, `plain.rs`,
      `block/map.rs`, `block/node.rs`; dead code in `input.rs::WithLimit`, `plain.rs::non_space_plain_chars`
      until Phase 1 wires it up) -- explicitly deferred until the phases above make real use of those
      parameters/types.
- [ ] Switch `spaces.rs::separate_lines`'s `alt` to `dispatch!` per its own TODO, for parity with
      `document.rs::yaml_stream`'s use of `dispatch!`.
- [ ] Public API ergonomics once the grammar is more complete: a top-level `ya::parse(&str) ->
      Result<value::Stream, _>` convenience function (today callers must reach into
      `parse::yaml_stream` + `parse::input::Input` + pick a winnow `Error` type themselves, as seen in
      `tests/integration_tests.rs`).
- [ ] Consider `serde` integration (feature-gated) once the value model round-trips real documents.
