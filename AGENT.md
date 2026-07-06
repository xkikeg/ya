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
    plain.rs            plain (unquoted) flow scalar content (ns-plain-*)
    scalar.rs           ties single/double quoted content to c-single-quoted / c-double-quoted
    directive.rs        Chapter 6.8: %YAML / %TAG / reserved directives
    document.rs         Chapter 9: l-yaml-stream, l-any-document, l-bare-document,
                        l-explicit-document, l-directive-document, document prefix/suffix;
                        public `yaml_stream()` entry point + its tests
    flow/
      content.rs         ns-flow-content / ns-flow-yaml-content / ns-flow-json-content
      node.rs             ns-flow-node / ns-flow-yaml-node / ns-flow-json-node
      seq.rs              c-flow-sequence and its entries
      map.rs              c-flow-mapping and its entries (explicit/implicit/yaml-key/json-key)
      pair.rs             ns-flow-pair (used for flow-sequence "single pair" shorthand entries)
    block/
      header.rs           block header: chomping/indentation indicators, auto-detected indentation
      literal.rs           c-l+literal (block literal scalar `|`)
      folded.rs            c-l+folded (block folded scalar `>`)
      scalar.rs           s-l+block-scalar (header + literal/folded + properties -> Node)
      seq.rs               l+block-sequence / c-l-block-seq-entry / ns-l-compact-sequence
      map.rs               l+block-mapping / ns-l-block-map-entry (explicit & implicit) /
                          ns-l-compact-mapping
      node.rs             s-l+block-node / s-l+block-in-block / s-l+block-collection /
                          s-l+flow-in-block / s-l+block-indented (all done)
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
- Aliases and the anchor store (`alias.rs`, `anchor.rs`), now populated: node properties
  (`properties.rs`) parse anchors and the three tag forms (verbatim, shorthand, non-specific), and
  `flow/node.rs` registers each anchored node into the anchor store as it's built, so
  `[&a foo, *a]`-style alias resolution works end-to-end.
- Node properties (`c-ns-properties`, `properties.rs`): anchors (`&name`), verbatim tags (`!<...>`),
  shorthand tags (`!handle!suffix`, including `%XX`-escaped suffixes), and the non-specific tag
  (`!`) alone. Tag handles (`tag_handles.rs`) resolve the two default handles (`!`, `!!`); an
  undeclared shorthand handle is a parse error. `value::Tag::NonSpecific` is a new variant for the
  bare-`!` case (forces str/map/seq, disables core-schema resolution later). Wired into
  `flow/node.rs`'s `flow_node`/`flow_yaml_node`/`flow_json_node`. See Phase 2 below (now complete
  for flow nodes; block-context property slots are still markers for their respective phases).
- Document stream skeleton for the *bare document* case (`document.rs::yaml_stream`,
  `bare_document`), with two working unit tests (flow seq, flow map).
- Plain scalars (`plain.rs`), one-line and multi-line, including line folding, the `#`-lookbehind
  and trailing-`:` rules of `ns-plain-char`, and the `c-forbidden` exclusion so a plain scalar can't
  swallow a `---`/`...` marker line (`document.rs::forbidden`). Produces `value::Scalar::Plain`, a
  new variant separate from `SingleStr`/`DoubleStr` since only plain-style scalars are eligible for
  core-schema resolution (Phase 6). See Phase 1 below (now complete).
- Block scalars, literal (`|`, `block/literal.rs`) and folded (`>`, `block/folded.rs`), including
  the chomping indicator/matrix (Strip/Clip/Keep, `block/header.rs`), the indentation indicator and
  auto-detected content indentation (`block/header.rs::detect_indentation`), folded-line vs.
  more-indented-line handling, and trailing comment lines. Produces the new `value::Scalar::Literal`
  / `Scalar::Folded` variants (both always resolve to `str`, like the quoted styles).
  `block/scalar.rs::block_scalar` ties header + content together, now also parses its own
  `c-ns-properties` and returns a full `Node` (not just a bare `Scalar`), and is reachable from real
  documents via `block_in_block`. See Phase 3 below (now complete).
- Block collections (`block/seq.rs`, `block/map.rs`, `block/node.rs`): block sequences and block
  mappings, both explicit (`? key` / `: value`) and implicit (`key: value`) entries, compact
  notation (`- - a`, `- key: value`), and `seq-space(n,c)` (a sequence value may align with its own
  mapping key's indentation). `block_node`/`block_in_block`/`block_indented` tie flow-in-block,
  block scalars, and block collections together into one recursive entry point, reachable from
  `document.rs::bare_document`. See Phase 4 below (now complete).
- Directives (`directive.rs`): `%YAML` (major-version-1 check, minor silently accepted),
  `%TAG` (registers a handle -> prefix mapping into the parse-time `TagHandles` map), and reserved
  (`%FOO ...`, consumed and ignored). Explicit documents (`document.rs::explicit_document`:
  `---` + bare document or an empty node) and directive documents (`directive_document`: one or
  more directives + an explicit document, with duplicate-`%YAML`/duplicate-handle detection) are
  both implemented and wired into `yaml_stream`'s dispatch, including per-document anchor/tag-handle
  reset (`AnchorStore::clear`/`TagHandles::clear`). See Phase 5 below (now complete).

Not implemented (stub returns `fail`, or missing outright) -- these are exactly the blockers a
`cargo build` warning-free pass would still leave semantically incomplete:
- Tag resolution / Core Schema (turning an unspecified-tag plain scalar into `Null`/`Bool`/`Int`/
  `Float`/string per the core schema) is deferred by design (see comment in `plain.rs::plain`) but
  nothing implements that later stage yet either.

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
  use `IndentLevel::initial()` / `IndentLevel::new(n)` / `.get()` / `.prev()` / `+ usize` rather than
  constructing the wrapped integer directly.
- The crate depends on `winnow` 1.0 (with the `simd` feature) and nothing else -- **keep it
  zero-dependency beyond winnow** (no `regex`, no `once_cell`; hand-write small matchers instead).
  When bumping winnow, check for combinator signature changes first.
- **Recursive grammar rules must break the construction cycle with a hand-rolled closure.**
  Combinators like `preceded(a, b)` *store* their sub-parsers eagerly, so an eager cycle
  (`flow_node` → `flow_content` → `flow_sequence` → `flow_node`) would be an infinitely-sized value.
  The existing code breaks each cycle by constructing sub-parsers *inside* a
  `move |input: &mut Input| { ... child(...).parse_next(input) }` closure body (see
  `flow/map.rs::flow_map_entries`, `single.rs::non_break_single_multi_line`). Block collections
  turned out to have *two independent* cycles, not one -- see Phase 4's writeup below for the one
  that isn't visually obvious from the spec composition (`block_map_implicit_value` → `block_node`
  directly, never passing through `block_indented`'s own closure). **When adding a new recursive
  rule, trace the whole call graph for cycles, not just the one the grammar's shape suggests.**
- winnow 1.0 idioms already in use, for reference when transcribing new rules: `alt`, `dispatch!`,
  `repeat` (note: `repeat(0.., p).map(|()| ())` to pick the `()` accumulator), `opt`, `preceded` /
  `terminated` / `delimited`, `peek`, `not` (peeks; succeeds at EOF), `empty.value(x)` (for `e-node`),
  `fail`, `.take()` (borrow the matched slice), `.with_taken()`, `.flat_map()`, `.verify()`,
  `.void()`, `.value()`, `.map()`.

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
update this list as work lands so it stays a reliable map of what's left.

**How to execute an item (checklist for a delegated agent):**

1. Read the linked spec section *in full* before writing code; the rule text (and its examples) is
   the requirements document.
2. Follow the conventions above: spec-transliterated names, `#[doc(alias)]`, spec-anchor doc link,
   `trace("module::name", ...)`, the generic `Context`/`Input`/`Error` signature shape, and the
   named "template" function each item points at.
3. Unit-test with the spec's own example strings, colocated `#[cfg(test)] mod tests`.
4. Run the full `cargo test`, then
   `cargo test --test integration_tests conformance_report -- --nocapture`; compare the pass count
   against the number recorded in Phase 7 below and **update that number** in the same change.
5. Update the checkboxes here.
6. Stop and ask the maintainer when you hit an "Escalate if" condition or one of the
   [open design questions](#open-design-questions-escalate-to-the-maintainer) at the bottom.

### Phase 0 -- Known bugs (small, self-contained; each is an independent task)

- [x] **Panic on empty input and on `...`-only input** (suite cases `AVM7`, `HWV9`; found by the
      Phase 7 harness). Root cause: winnow 1.0 asserts "`repeat` parsers must always consume", and
      `spaces.rs::line_comment` *can* succeed consuming nothing at EOF -- `separate_in_line`'s
      start-of-line escape hatch (`spaces.rs:173-176`) succeeds empty, and `break_comment`'s `eof`
      arm (`spaces.rs:438`) succeeds empty. That empty-success reaches two `repeat`s:
      `document.rs::document_prefix`'s `repeat(0.., spaces::line_comment)` (`document.rs:153`, trips
      on empty input) and `yaml_stream`'s `repeat(0.., document_prefix)` (`document.rs:37`, trips
      after a `...` suffix at EOF, because `document_prefix` itself can succeed empty).
      Fix: guard both repeat bodies to require consumption, e.g.
      `repeat(0.., p.take().verify(|s: &&str| !s.is_empty()).void())`, or a hand-rolled loop that
      breaks when `.with_taken()` returns empty (the pattern already used in
      `spaces.rs::line_comments` and `yaml_stream`'s outer loop). Add `yaml_stream` unit tests for
      `""` and `"..."` (both should parse as an empty stream, not panic).
- [x] **Double-quoted scalar drops a tab adjacent to an escaped space** (suite cases named
      "Trailing tabs in double quoted", see conformance report). Diagnose in
      `double.rs::non_break_double_multi_line`'s fold/trim step: when trimming trailing `s-white`
      before a folded break, an *escaped* whitespace (`\ ` / `\t`) must not be treated as trimmable,
      and a real tab next to one must survive. Reproduce with the two corpus cases and spec
      example 7.5 before fixing.
- [x] **`block/seq.rs:55` lookahead is inverted.** Spec
      [`c-l-block-seq-entry`](https://yaml.org/spec/1.2.2/#rule-c-l-block-seq-entry) is
      `'-' [lookahead ≠ ns-char] ...` (the char after `-` must NOT be non-space, else it's a plain
      scalar like `-foo`), but the code has `peek(one_of(chars::is_non_space))`, i.e. requires that
      it IS. Fix: `winnow::combinator::not(one_of(chars::is_non_space))` (`not` peeks and also
      succeeds at EOF). Currently masked because `block_indented` is a `fail` stub; must land
      before/with Phase 4.
- [x] **`key.rs:31-33` / `key.rs:64-67`: trailing `s-separate-in-line` must be optional.** Spec
      [`c-s-implicit-yaml-key`](https://yaml.org/spec/1.2.2/#rule-c-s-implicit-yaml-key) /
      `c-s-implicit-json-key` end with `s-separate-in-line?` (optional), but the code requires it.
      Consequence today: a flow pair key directly followed by `:` (e.g. `[a: b]` once plain scalars
      work, or `["a": b]` already) fails because there's no whitespace before the `:`. Fix: wrap in
      `opt(...)`. Note the same helpers get reused for block mapping implicit keys in Phase 4.
- [x] Cosmetic: `block/node.rs:68` `trace` label inside `flow_in_block` says
      `"block::node::block_in_block"` (copy-paste); `block/header.rs:19` `#[doc(alias)]` on
      `chomping_indicator` says `"l+block-mapping"` (should be `"c-chomping-indicator"`).

### Phase 1 -- Plain scalars (highest priority: most common YAML scalar style) -- DONE

All in `plain.rs` unless noted. The already-working `non_space_plain_first` (`plain.rs:76`) and the
context plumbing (`FlowOrKey::is_plain_safe`, `FlowOrKey::non_space_plain` in `context.rs`) stay
as-is; only the stubs and the one-line/multi-line composition change.

- [x] **1a. Lookbehind support in `input.rs`.**
      [`ns-plain-char`](https://yaml.org/spec/1.2.2/#rule-ns-plain-char) has a
      `[lookbehind = ns-char] '#'` alternative; winnow has no lookbehind, but this codebase already
      solved the same problem once: `TrackStartOfLine` (`input.rs:76-90`) inspects the byte before
      the cursor via `LocatingSlice::previous_token_end()` + the saved `original` slice. Add
      `fn previous_char(&self) -> Option<char>` the same way (either extend `TrackStartOfLine` or a
      sibling trait): `self.original[..self.inner.previous_token_end()].chars().next_back()`,
      `None` at offset 0. Delegate on `WithLimit`, and add the bound to the `InputStream` trait
      alias *and* its blanket impl (`input.rs:11-31`).
- [x] **1b. `ns-plain-char` (`non_space_plain_chars`).** Change the stub's return type from
      `&'i str` to `char` (it's a single-char rule) and implement the spec's three alternatives as
      an `alt`:
      1. `one_of(|c| Context::is_plain_safe(c) && c != ':' && c != '#')`;
      2. `'#'` guarded by `previous_char()` being an `ns-char` (hand-rolled closure: check
         `input.previous_char().is_some_and(chars::is_non_space)`, then parse `'#'`);
      3. `terminated(one_of(':'), peek(one_of(Context::is_plain_safe)))` -- same shape as
         `non_space_plain_first`'s second arm (`plain.rs:88`).
      Note the lookbehind arm is self-consistent everywhere it's reachable: mid-line after a plain
      char it fires (`foo#bar` is one scalar), after whitespace or a line break the previous char
      isn't `ns-char` so ` #...` correctly stays a comment.
- [x] **1c. `nb-ns-plain-in-line` (new fn `non_break_non_space_plain_in_line(context)`).**
      Spec: `( s-white* ns-plain-char(c) )*`. Transcribe literally:
      `repeat(0.., (take_while(0.., chars::is_white_space), non_space_plain_chars(context))).map(|()| ()).take()`
      returning borrowed `&'i str`. Trailing-whitespace correctness comes free: `repeat` backtracks
      the whole failed iteration, so in `foo  # comment` the two spaces before `#` are left
      unconsumed. Each successful iteration consumes ≥ 1 char (the plain char), so the
      repeat-must-consume invariant holds even when `s-white*` is empty.
- [x] **1d. `ns-plain-one-line` (`non_space_plain_one_line`).** Replace "first char only" with the
      spec composition: `(non_space_plain_first(context), non_break_non_space_plain_in_line(context)).take()`.
      Landed with its `Context` bound relaxed from `Key` to `FlowOrKey`, since 1f now also calls it
      for `FlowIn`/`FlowOut` (not just the two key contexts).
- [x] **1e. `s-ns-plain-next-line` (new fn `space_non_space_plain_next_line(context, indent_level)`).**
      Spec: `s-flow-folded(n) ns-plain-char(c) nb-ns-plain-in-line(c)` -- note the continuation line
      starts with `ns-plain-char`, *not* `ns-plain-first`. Compose from `spaces::flow_folded`
      (`spaces.rs:351`), 1b and 1c; return the fold `Cow` plus the line text so the caller can
      concatenate.
- [x] **1f. `ns-plain-multi-line` (`non_space_plain_multi_line`).** Mirror
      `single.rs::non_break_single_multi_line`'s hand-rolled loop (`single.rs:40-68`): start with
      `Cow::Borrowed` of the one-line parse, then loop `opt(space_non_space_plain_next_line(...))`,
      pushing fold string + line text into `current.to_mut()`. Simpler than the single-quoted
      version: no trailing-`s-white` trim step is needed, because 1c guarantees a plain line never
      ends in whitespace.
- [x] **1g. Document-marker exclusion.** A multi-line plain scalar must not swallow a `---`/`...`
      line ([`c-forbidden`](https://yaml.org/spec/1.2.2/#rule-c-forbidden), excluded from
      `l-bare-document`). Added `document.rs::forbidden` (start-of-line + `---`|`...` + followed by
      break/white/EOF, using `spaces::start_of_line`) and `not(forbidden)` before parsing each
      continuation line in `space_non_space_plain_next_line` (1e). Turned out this wasn't only a
      continuation-line concern: a *lone* `...`/`---` as an entire bare document (e.g. the `"..."`
      stream test from Phase 0) would otherwise parse as a one-line plain scalar and swallow the
      document-end marker on the very first line, so `plain()` itself also gates on
      `not(forbidden)` before calling `Context::non_space_plain`.
- [x] **1h. New `value::Scalar::Plain(Cow<'i, str>)` variant**, and map `plain()` (`plain.rs:34`) to
      it instead of `Scalar::SingleStr`. Required so Phase 6 can distinguish plain (schema-resolvable)
      from single-quoted (always `str`). Updated the `Scalar` → `ExpectedNode` conversion in
      `tests/integration_tests.rs` and the unit tests that previously asserted `SingleStr` for plains
      (`key.rs`'s implicit-yaml-key test).
- [x] **1i. Tests.** Spec example 7.12 "Plain Lines" (verified against the built spec HTML --
      AGENT.md's original "7.9-7.12" range was off; §7.3.3 only has two examples, "Plain Implicit
      Keys" (7.11) and "Plain Lines" (7.12)) from
      [§7.3.3](https://yaml.org/spec/1.2.2/#733-plain-style); edge fixtures: `::vector`, `-123`,
      `foo#bar` (one scalar) vs `foo #bar` (scalar + comment), `a:b` (one scalar), `key:` boundary
      (stops before `:`), a document-end-marker regression case. Added an end-to-end `document.rs`
      test, `[one, two]`. Conformance report updated in Phase 7 below: 118/402 (29%) -> 129/402
      (32.1%).

Escalate if: the `previous_char` lookbehind turns out to conflict with `WithLimit` semantics or
another input wrapper -- the fallback design (thread a "previous char class" flag through a fused
in-line loop) trades away spec shape and should be a maintainer decision.

### Phase 2 -- Node properties (anchors & tags) -- DONE (for flow nodes)

- [x] **2a. Character classes in `chars.rs`**: `is_word_char`
      ([`ns-word-char`](https://yaml.org/spec/1.2.2/#rule-ns-word-char): alnum + `-`),
      `is_tag_char` ([`ns-tag-char`](https://yaml.org/spec/1.2.2/#rule-ns-tag-char)), and a
      `uri_chars` slice parser for [`ns-uri-char`](https://yaml.org/spec/1.2.2/#rule-ns-uri-char) --
      URI chars include `%xx` hex escapes, so a plain predicate isn't enough; use
      `repeat(1.., alt((one_of(<plain uri chars>).void(), ('%', hexdig, hexdig).void()))).take()`.
      Keep escapes *raw* (don't percent-decode) at parse time; decoding is a resolution concern.
- [x] **2b. New `parse/properties.rs`.** Output types:
      `struct Properties<'i> { anchor: Option<&'i str>, tag: Option<TagProperty<'i>> }`,
      `enum TagProperty<'i> { Verbatim(&'i str), Shorthand { handle: &'i str, suffix: &'i str }, NonSpecific }`
      (conversion to `value::Tag` happens at the node-wiring layer, 2d, using the handle map from 2c).
      Rule functions:
      - `properties` ([`c-ns-properties(n,c)`](https://yaml.org/spec/1.2.2/#rule-c-ns-properties)):
        `alt(( (tag_property, opt(preceded(separate, anchor_property))), (anchor_property, opt(preceded(separate, tag_property))) ))`.
      - `tag_property` (`c-ns-tag-property`): `alt((verbatim_tag, shorthand_tag, non_specific_tag))`
        -- order matters, bare-`!` last.
      - `verbatim_tag` (`c-verbatim-tag`): `delimited("!<", uri_chars, '>')`.
      - `shorthand_tag` (`c-ns-shorthand-tag`): `(tag_handle, take_while(1.., is_tag_char))`.
      - `non_specific_tag` (`c-non-specific-tag`): `'!'`.
      - `tag_handle` (`c-tag-handle`): `alt((named, "!!", "!"))` with named
        (`c-named-tag-handle` = `'!' ns-word-char+ '!'`) first; `.take()` the whole handle.
      - `anchor_property` (`c-ns-anchor-property`): `preceded('&', anchor_name)`;
        `anchor_name` (`ns-anchor-name`): `take_while(1.., |c| chars::is_non_space(c) && !chars::is_flow_indicator(c))`
        (same predicate as `alias.rs:24`).
- [x] **2c. Tag-handle → prefix map in parse state.** `Input`'s state is currently `AnchorStore`
      directly (`input.rs:41`). Generalize: `Stateful<LocatingSlice<&str>, ParseState<'i>>` where
      `ParseState { anchors: AnchorStore<'i>, tag_handles: TagHandles<'i> }`; add a `WithTagHandles`
      trait mirroring `WithAnchorStore` (`input.rs:57`) and add it to the `InputStream` alias bounds.
      `TagHandles`: map with defaults per [§6.9.1](https://yaml.org/spec/1.2.2/#rule-c-tag-handle):
      `!` → `!` (local) and `!!` → `tag:yaml.org,2002:`; `%TAG` (Phase 5) inserts more. Shorthand
      resolution = prefix + suffix → `value::Tag::Global(Cow::Owned(...))` for now; mapping
      well-known `tag:yaml.org,2002:*` URIs to `Tag::Standard` is Phase 6's job. For `NonSpecific`
      (`!`), `value::Tag` needs a way to say "explicitly non-specific" (forces str/map/seq at
      resolution, unlike `Unspecified` which allows plain-scalar schema resolution) -- `value.rs:61`
      has a TODO asking exactly this; add a `Tag::NonSpecific` variant (escalate if in doubt).
- [x] **2d. Wire into `flow/node.rs`** (replace the three `// TODO: fixme Support properties.`):
      - `flow_yaml_node` ([rule](https://yaml.org/spec/1.2.2/#rule-ns-flow-yaml-node)): third alt arm
        `(properties(context, n), opt(preceded(separate, flow_yaml_content)))` -- **properties with
        no content is legal** (`!!str &a` alone) and yields `Content::Empty` with that tag/anchor.
      - `flow_json_node`: `(opt((properties, separate)), flow_json_content)`.
      - `flow_node`: `alias_node | flow_content | (properties, opt(preceded(separate, flow_content)))`.
      - **Anchor registration**: after the full `Node` is built, if an anchor was present, register
        `input.anchor_store_mut().put(name.to_string(), node.clone())`. This needs `input` access,
        so write the property-carrying arms as hand-rolled closures (pattern: `key.rs:27`,
        `single.rs:49`), not pure combinator chains.
- [x] **2e. Property slots in `block/scalar.rs` and Phase 4's `block_in_block`** are marked in those
      phases; this phase landed first (no block phase yet), so nothing else to do here -- whichever
      block phase lands next should wire `properties(...)` into those slots directly rather than
      leaving further `// TODO(Phase 2)` markers.
- [x] **2f. Tests.** Anchor/tag property tests colocated in `properties.rs` (verbatim incl. the
      bare-`!` invalid case, shorthand incl. `%XX`-escaped suffix and undeclared-handle rejection,
      non-specific, anchor-only, and both properties orderings) and `tag_handles.rs` (default
      handles, registering a named handle); end-to-end in `flow/node.rs` (tag+anchor together,
      anchor-only defaulting to `Unspecified`, tag-only with empty content, JSON-node tag, the
      non-specific `!` tag, and the undeclared-handle parse error) and `document.rs`
      (`[&a foo, *a]` parses to two equal `foo` nodes via anchor/alias resolution). Full literal
      spec-example transcriptions (6.23-6.29) were not added as separate tests since they all
      exercise the same flow-node property grammar already covered above; revisit if a future phase
      needs example-level fixtures for regression tracking.

### Phase 3 -- Block scalars (literal `|` / folded `>`) -- DONE

- [x] **3a. `block/header.rs`: `indentation_indicator` + `block_header`.**
      [`c-indentation-indicator`](https://yaml.org/spec/1.2.2/#rule-c-indentation-indicator):
      `opt(one_of('1'..='9')).map(|c| c.map(|c| c as usize - '0' as usize))`.
      [`c-b-block-header`](https://yaml.org/spec/1.2.2/#rule-c-b-block-header) allows the two
      indicators *in either order*, then `s-b-comment`: simplest faithful shape is
      `(opt(ind), chomping_indicator, opt(ind))` + verify not both `Some` (or `alt` of the two
      orders; note `chomping_indicator` never fails, it's `opt`-based). Return
      `(Option<usize>, ChompingMode)`; end with `spaces::space_break_comment`.
- [x] **3b. Auto-detected indentation** ([§8.1.1.2](https://yaml.org/spec/1.2.2/#rule-c-indentation-indicator),
      the hard part of this phase). Implemented as a hand-rolled `detect_indentation` that
      `checkpoint()`s, scans forward line-by-line counting leading spaces (empty = only spaces then
      break), `reset()`s, and returns the absolute `IndentLevel` -- the actual content parse then
      re-consumes normally. **Deviation from this plan's literal wording**: the function takes no
      `n` parameter at all (not `detect_indentation(n)`), and doesn't compute an `n`-relative `m`.
      The actual spec text for this rule (unlike the indentation-indicator case) defines the
      content indentation level as simply "the leading spaces of the first non-empty line" (or the
      longest line, if none) -- an absolute quantity, not `n + m`. Computing it via `n`-relative
      arithmetic turns out to be actively wrong at the document-root sentinel (`IndentLevel`'s
      internal `n+1` representation collapses `-1` and `0` under `.get()`'s `saturating_sub`),
      so the simpler, more literal reading was also the correct one. Error if any leading *empty*
      line is more indented than that first non-empty line. Known scope limitation documented
      inline: the scan isn't bounded by the block scalar's own indentation level, so an empty block
      scalar immediately followed by unrelated, less-indented sibling content could have that
      sibling's indentation misread as detected content indentation; harmless today since
      `block_scalar` is only unit-tested directly (3f) -- flagged for Phase 4 to revisit if it
      surfaces there. Tested with spec examples 8.2 and 8.3 (8.3's invalid over-indented-empty-line
      case; its other invalid case, an under-indented continuation line, self-heals into an empty
      scalar that leaves the foreign line unconsumed rather than erroring, which is fine given
      `block_scalar` isn't embedded in real document parsing yet).
- [x] **3c. New `block/literal.rs`.** Rules, 1:1:
      - `literal_text` ([`l-nb-literal-text(n)`](https://yaml.org/spec/1.2.2/#rule-l-nb-literal-text)):
        `(repeat(0.., spaces::line_empty(BlockIn, n)), spaces::indent(n), take_while(1.., chars::is_non_break))`
        -- the collected empties contribute `\n`s.
      - `literal_next` (`b-nb-literal-next(n)`): `preceded(chars::break_as_line_feed, literal_text(n))`.
      - Chomping helpers (here or `header.rs`): `chomped_last`
        ([`b-chomped-last(t)`](https://yaml.org/spec/1.2.2/#rule-b-chomped-last); Strip → break or
        EOF contributes nothing, Clip/Keep → `\n`; careful with the no-final-break EOF case) and
        `chomped_empty` (`l-chomped-empty(n,t)`: Keep → trailing `l-empty*` kept as `\n`s, else
        discarded; includes `l-trail-comments`).
      - `literal_content` (`l-literal-content(n,t)`): `opt((literal_text, repeat(0.., literal_next), chomped_last))` + `chomped_empty`.
      - `literal` ([`c-l+literal(n)`](https://yaml.org/spec/1.2.2/#rule-c-l+literal)): hand-rolled:
        parse `'|'`, `block_header`, resolve `m` (explicit or 3b), then
        `literal_content(n + m, t)`. Output is folded/joined → `Cow::Owned` in general.
- [x] **3d. New `block/folded.rs`.** The most intricate rule cluster; transcribed each of
      `s-nb-folded-text` / `l-nb-folded-lines` / `s-nb-spaced-text` / `b-l-spaced` /
      `l-nb-spaced-lines` / `l-nb-same-lines` / `l-nb-diff-lines` /
      [`l-folded-content`](https://yaml.org/spec/1.2.2/#rule-l-folded-content) 1:1. Key semantics:
      breaks between same-indented text lines fold to a space (reuse
      `spaces::break_line_folded(BlockIn, ...)`), but "more-indented" lines (spaced text, starting
      with extra white) are kept literal with real breaks. Tested each sub-rule (via the composed
      `folded()` entry point) against spec examples 8.8, 8.10 and 8.11.
- [x] **3e. `block/scalar.rs::block_scalar`**: per
      [`s-l+block-scalar`](https://yaml.org/spec/1.2.2/#rule-s-l+block-scalar), minus the
      properties part (see below): `preceded(separate(context, n+1), alt((literal(n).map(Scalar::Literal), folded(n).map(Scalar::Folded))))`.
      Added `value::Scalar::Literal(Cow)` and `Scalar::Folded(Cow)` variants (block scalars always
      resolve to `str` in Phase 6, like quoted); updated the harness conversion in
      `tests/integration_tests.rs`. **Deviation**: did *not* wire in `c-ns-properties(n+1,c)` here
      as the plan's composition shows -- doing so would require `block_scalar` to return a `Node`
      (to carry the anchor/tag) instead of a bare `Scalar`, and that signature change is already
      planned as part of Phase 4a's wider Content->Node migration for block constructs. Left a
      `TODO(Phase 4)` comment at the spot instead of doing it piecemeal here.
- [x] **3f.** `block_scalar` is unreachable until Phase 4's `block_in_block` dispatches to it; until
      then, unit-tested directly (`block/scalar.rs`'s own tests, plus each of `literal.rs`/
      `folded.rs`/`header.rs`'s colocated tests exercising the sub-rules directly).
- [x] **3g. Tests.** Spec examples 8.5 (chomping matrix, both literal and folded halves), 8.7
      (Literal Scalar), 8.8 (Folded Scalar), 8.9 (Literal Content), 8.10 (Folded Lines, adapted --
      see the test's doc comment for what was dropped and why), 8.11 (More Indented Lines), plus
      the block header composition itself (empty/indentation-only/chomping-only/both-orders) and
      `detect_indentation`'s own success/error cases. Two real, previously-latent bugs surfaced and
      were fixed along the way (both outside this phase's own new code, but required for it to
      work): `chars::is_non_break` excluded space (`char::is_ascii_graphic` only covers `x21-x7E`,
      not space at `x20`), breaking any multi-word comment or block-scalar content line -- fixed to
      `is_ascii() && !is_ascii_control()`; and a new `repeat(0.., spaces::line_comment)` call in
      `l-trail-comments` hit the exact same "`repeat` must always consume" trap Phase 0 already
      fixed in `document.rs` (`line_comment` can succeed while consuming nothing at EOF), fixed with
      the same `.take().verify(|s| !s.is_empty())` guard. Conformance report: 129/402 (32.1%) ->
      132/402 (32.8%) -- the small gain is from the `is_non_break` fix (comments/content with
      spaces), not from block scalars themselves, since they're not yet reachable from real
      documents (see 3f); no regressions (`StructuralMismatch`/`ParserPanic`/
      `UnexpectedSuccessOnErrorCase` all still 0, `ContentMismatch` still 2).

### Phase 4 -- Block collections (biggest conformance jump; needs Phases 1 & 3, property slot from 2) -- DONE

- [x] **4a. Prerequisite signature fix.** `block_seq_entry` / `block_sequence` / `block_indented`
      now produce `Node<'i>` / `Vec<Node<'i>>` (`block/seq.rs`, `block/node.rs`) instead of
      `Content` / `Vec<Content>`; property-less compact collections wrap with `Node::unspecified`.
      `block_scalar` (Phase 3) was widened the same way, from `Scalar<'i>` to `Node<'i>` -- see 4f.
- [x] **4b.** The Phase 0 `block/seq.rs` inverted-lookahead fix and the `key.rs` `opt()` fix had
      already landed by the time this phase started (both checked off in Phase 0 above).
- [x] **4c. `block/map.rs::block_map_entry`** implemented as planned:
      `alt((block_map_explicit_entry(n), block_map_implicit_entry(n)))`, with
      `block_map_explicit_key`/`block_map_explicit_value` wrapping `block_indented(BlockOut, n)`,
      and `block_map_implicit_key` = `alt((key::implicit_json_key(BlockKey),
      key::implicit_yaml_key(BlockKey)))`. `e_node` (`empty.value(Node::unspecified(Content::Empty))`)
      landed in `block/node.rs` (`pub(super)`, so `block::map`/`block::seq` can reuse it) rather than
      `flow/map.rs`'s copy. **Deviation from the plan**: `block_map_implicit_value` (`c-l-block-map-
      implicit-value`) had to become its own hand-rolled closure -- see 4d's recursion note, this
      function turned out to sit on a *second*, independent recursion cycle back to `block_node`
      that the planned `block_indented` closure alone doesn't cover.
- [x] **4d. `block/node.rs::block_indented`** implemented per plan:
      `alt((compact_notation(n), block_node(context, n), terminated(e_node, line_comments)))`,
      `compact_notation` consuming `s-indent(m)` for arbitrary `m` then dispatching to
      `compact_sequence`/`compact_mapping` at `n' = indent_level + (m + 1)`.
      **The recursion turned out to have two independent cycles, not one.** The planned one
      (`block_indented` → `compact_notation` → `compact_sequence`/`compact_mapping` →
      `block_seq_entry`/`block_map_entry` → `block_indented` again, e.g. for `- - a` / `- a: b`) is
      broken by making `block_indented` itself the hand-rolled closure, exactly as planned. But a
      *second*, disjoint cycle exists that never passes through `block_indented` at all:
      `block_node` → `block_in_block` → `block_collection` → `block_mapping` → `block_map_entry` →
      `block_map_implicit_entry` → `block_map_implicit_value` → `block_node` again (an ordinary
      `key: value` mapping entry, no compact notation involved). Missing this second closure is a
      quiet failure mode: the code *compiles* (opaque-type resolution doesn't require passing
      through the missing closure to terminate) but is unboundedly self-referential in a way that
      only some particular reasoning about closures-vs-plain-combinators catches ahead of time; it
      was caught here by construction rather than by a compile error, so a future recursive rule
      addition should re-derive the *whole* call graph's cycles, not just the one the spec
      composition makes visually obvious. Fixed by also making `block_map_implicit_value` (in
      `block/map.rs`) a hand-rolled closure.
- [x] **4e. `ns-l-compact-sequence(n)` / `ns-l-compact-mapping(n)`** landed in `block/seq.rs` /
      `block/map.rs` exactly as planned.
- [x] **4f. `block/node.rs::block_in_block`**: `alt((scalar::block_scalar(context, n),
      block_collection(context, n)))`. Per the plan, `block_scalar` itself now parses
      `c-ns-properties(n+1,c)` directly (deferred from Phase 3) and returns `Node<'i>`, so the
      "scalar arm" needed no extra wrapping. `block_collection` transcribes
      `s-l+block-collection(n,c)` as planned, hand-rolled as a closure (needed regardless of
      recursion, to imperatively register the optional anchor via the shared `properties::build_node`
      helper -- see below). `seq-space(n,c)` landed as `InOutBlock::seq_space` exactly as planned
      (`BlockIn` → identity, `BlockOut` → `.prev()`).
      **Refactor bundled in**: `flow/node.rs`'s private `build_node` (tag resolution + anchor
      registration) moved to `properties::build_node` (`pub(super)`, i.e. visible to all of
      `parse::*`) so `block_scalar` and `block_collection` could reuse it instead of duplicating
      the resolve-tag/register-anchor dance a third and fourth time.
- [x] **4g. Tests.** Colocated unit tests transcribing spec examples 8.14 (`block/seq.rs`), 8.16,
      8.17, 8.19 (`block/map.rs`), plus one end-to-end `document.rs` test for the full example 8.14
      (mapping-of-sequence-of-mapping, exercising `seq-space` and full document dispatch --
      something the function-level tests can't cover on their own). Simpler `key: value\n` /
      `- a\n- b\n` end-to-end cases were deliberately *not* added at the document level, per review
      feedback on the first version of this change: they're already fully covered by the
      block-sequence/block-mapping-level tests, so a redundant document-level copy would just be
      extra maintenance surface. All nine yaml-test-suite corpus cases tagged with spec examples
      8.14-8.22 (`JQ4R`, `W42U`, `TE2A`, `5WE3`, `S3PD`, `V9D5`, `735Y`, `M5C3`, `57H4`) now pass
      under the strict harness. Conformance report: 132/402 (32.8%) → **280/402 (69.7%)**, by far
      the largest jump of any phase so far, as predicted.

**Bugs found and fixed along the way** (all were latent, pre-existing gaps that Phase 4 was simply
the first phase to reach at runtime -- none are new mistakes introduced by this phase's own new
code, but all were blockers for it):
- **`spaces::line_comments` (`s-l-comments`) didn't implement "zero or more" correctly.** Its
  hand-rolled loop (written to dodge the Phase-0-style "`repeat` must always consume" trap) called
  `line_comment` and propagated *any* failure with `?`, instead of treating a failed match as "stop
  the loop, there are zero further comment lines" the way `l-comment*` requires. This is harmless
  for a single flow-only document (the only shape exercised before Phase 4) but breaks *every*
  multi-entry block collection: after parsing one entry's value, `flow_in_block`'s trailing
  `line_comments` call would try to consume the next entry's line as a would-be comment, fail, and
  propagate that failure instead of gracefully leaving it for the next entry to parse. Fixed by
  checkpointing before the trailing-comment attempt and resetting on a backtrackable failure.
- **`spaces::indent_less_than` used `Error::assert` (a debug-mode `panic!`) for `n<=0`,** on the
  reasoning that "less than zero spaces" should never be reachable. It is reachable: a block
  scalar's auto-detected content indentation can legitimately be 0 (e.g. a root-level literal/folded
  scalar), and its trailing-empty-lines handling (`l-trail-comments`, always wrapped in `opt(...)`)
  calls this function unconditionally. Fixed by returning an ordinary backtrackable error instead of
  panicking -- both call sites already handle that gracefully (`line_empty`'s `line_prefix` fallback
  never actually needs it at `n=0`; `l-trail-comments`'s `opt(...)` wrapper treats it as "no trailing
  comment").
- **`block/header.rs::detect_indentation` was unbounded**, exactly as flagged as a known limitation
  when Phase 3 landed it: a block scalar with no content of its own, followed immediately by a
  sibling at or below its own indentation (e.g. yaml-test-suite `K858`'s `strip: >-` immediately
  followed by `clip: >` at column 0), would misread that sibling's line as its own auto-detected
  content indentation and swallow it. Fixing this needed two changes, not one: (1) bound the
  forward scan by the scalar's own indentation level `n`, so a non-empty candidate line indented at
  or below `n` no longer counts as content; (2) *also* teach `literal_content`/`folded_content` (via
  a new `DetectedIndentation { content: Option<IndentLevel>, bound: IndentLevel }` return type) to
  skip the text-line-matching phase entirely when there's no content, rather than attempting it at
  a computed level that can itself be a degenerate `s-indent(0)` (which matches trivially,
  consuming zero spaces unconditionally, and so would swallow the sibling line anyway even knowing
  "there's no content" if the matching phase were still attempted). The `bound` field is still
  needed even in the no-content case, to correctly recognize deeply-indented blank lines as blank
  during the trailing chomping phase.

### Phase 5 -- Directives & explicit/full documents -- DONE

- [x] **5a. New `parse/directive.rs`**. Landed close to plan, with one structural deviation (see
      below): `directive` ([`l-directive`](https://yaml.org/spec/1.2.2/#rule-l-directive)) is
      `terminated(preceded('%', directive_body), spaces::line_comments)`; `yaml_directive`
      (`ns-yaml-directive`) + `yaml_version` (`ns-yaml-version`: `dec_digits '.' dec_digits` →
      `(u32, u32)`) hard-errors on major ≠ 1 and silently accepts any minor version (no warning
      channel exists, noted in the doc comment); `tag_directive` (`ns-tag-directive`) parses
      `"TAG"`, `properties::tag_handle` (widened from private to `pub(super)` so this sibling
      module could reuse it), and `tag_prefix` (`ns-tag-prefix`: `local_tag_prefix` = `!` +
      `ns-uri-char*`, or `global_tag_prefix` = a tag char + `ns-uri-char*`), returning a `Directive`
      value for the caller to register/validate (registration itself needs `&mut Input`, which
      `directive()` doesn't have, so it stays the caller's job -- see 5c); `reserved_directive`
      (`ns-reserved-directive`) consumes a name + optional parameters and is ignored.
      **Deviation**: `directive` does *not* dispatch via a plain
      `alt((yaml_directive, tag_directive, reserved_directive))` as planned. `reserved_directive`'s
      name grammar (`ns-char+`) also matches the literal `"YAML"`/`"TAG"`, so with a backtracking
      `alt`, a malformed `%YAML`/`%TAG` body (e.g. `%YAML 2.0`, an unsupported major version) would
      silently backtrack out of `yaml_directive` and get reinterpreted by `reserved_directive` as
      an unrelated, ignored directive -- accepting exactly the input the version check exists to
      reject. Fixed with a small hand-rolled `directive_body` that peeks the directive name first
      (via `peek(directive_name)`) and dispatches to `yaml_directive`/`tag_directive`/
      `reserved_directive` by exact-match, so once the name is unambiguously `"YAML"` or `"TAG"`,
      any further failure is a hard error rather than a fall-through. (A `cut_err`-based fix was
      tried first and reverted: `cut_err` requires `Error: ModalError`, which this crate's shared
      `ParserError` trait alias can't add without breaking every other caller -- `winnow::Result<O,
      E>` is a bare `Result<O, E>`, not `Result<O, ErrMode<E>>`, so `ContextError` itself (the
      concrete type used everywhere, including `tests/integration_tests.rs`) never implements
      `ModalError`, only `ErrMode<ContextError>` does.)
- [x] **5b. `document.rs::explicit_document`**: `preceded(directives_end, alt((bare_document,
      terminated(empty.value(Node::unspecified(Content::Empty)), spaces::line_comments).map(
      Document::new))))`, plus a `directives_end` rule fn (`c-directives-end` = literal `"---"`).
      Matches the plan; same-line (`--- foo`) and next-line (`---\nfoo`) content both work through
      `bare_document`'s existing separation handling, no changes needed there.
- [x] **5c. `document.rs::directive_document`**. Not the planned
      `preceded(repeat(1.., directive).map(|()| ()), explicit_document)`: a plain `repeat` can't
      register a `%TAG` handle (needs `&mut Input`) or detect a duplicate `%YAML`/handle (needs
      state across iterations) from inside a combinator chain, so it's a hand-rolled loop that
      calls `directive::directive` once per line, tracks `seen_yaml: bool` and
      `seen_handles: HashSet<&str>`, calls `input.tag_handles_mut().put(...)` for each accepted
      `%TAG`, and returns a hard `Err` (with a `StrContext` message) the moment either is violated,
      before falling through to `explicit_document` once at least one directive parsed. Verified
      against spec example 6.15 ("Invalid Repeated TAG Directive") and an analogous duplicate-
      `%YAML` case, both via direct `testing::parse(directive_document, ...)` calls (going through
      `yaml_stream` wouldn't observe the failure -- see 5d's note on why).
- [x] **5d. `yaml_stream`'s dispatch**: the `'-'` arm now calls the real `explicit_document`, as
      planned. **The planned `'%' => directive_document.map(Some)` arm was added, tested, then
      *removed* again -- it was wrong.** `l-yaml-stream`'s grammar is
      `prefix* any_document? ( suffix+ prefix* any_document? | prefix* explicit_document? )*`: each
      loop iteration is *either* the suffix branch (peeked `.`: one or more `...`, then *any* kind
      of document) *or* the no-suffix branch (peeked `-`: *only* an explicit, `---`-prefixed
      document -- never a fresh directive or bare document without an intervening `...`). A
      standalone `'%'` arm let a directive start a *second* document with no preceding `...`, which
      is exactly what corpus cases `9HCY` ("Need document footer before directives"), `EB22`
      ("Missing document-end marker before directive"), `RHX7` ("YAML directive without document
      end marker") and `MUS6/01` ("Directive variants") say must be rejected -- adding the arm
      turned all four from correctly-erroring into `UnexpectedSuccessOnErrorCase` (previously 0,
      per Phase 7). Removed the arm entirely: a bare `'%'` at loop-top now falls to the catch-all
      arm, fails to parse as a comment, and correctly ends the stream leaving it unconsumed. A
      directive is still reachable exactly where the grammar allows it: as the very first document
      (the `initial` computation before the loop) or right after a `...` suffix (already inside the
      `'.'` arm's `opt(any_document)`, unchanged).
- [x] **5e. Per-document state reset.** Added `AnchorStore::clear()` (`self.0.clear()`) and
      `TagHandles::clear()` (`*self = Self::new()`, restoring just the two default handles), and a
      small `reset_document_state` parser in `document.rs` that calls both; wired via
      `preceded(reset_document_state, any_document)`/`preceded(reset_document_state,
      explicit_document)` at every document-boundary call site in `yaml_stream` (the initial
      document, the `'.'` arm, and the `'-'` arm). Phase 1g's `c-forbidden` exclusion was already
      landed in Phase 1, but turned out to be *incomplete*: it only covered plain scalars.
      Multi-line **double-quoted and single-quoted** scalars had no equivalent guard, so a quoted
      scalar could swallow a `---`/`...` marker line as ordinary content across a fold -- corpus
      cases `5TRB` ("Invalid document-start marker in doublequoted string"), `RXY3` ("Invalid
      document-end marker in single quoted string") and `9MQT/01` ("Scalar doc with '...' in
      content") all rely on this being rejected, and adding real `explicit_document` support (5b)
      was what first made them reachable enough to expose it as `UnexpectedSuccessOnErrorCase`
      rather than a total parse failure for an unrelated reason. Fixed the same way Phase 1g fixed
      plain scalars: in `double::non_break_double_multi_line` and
      `single::non_break_single_multi_line`, each fold alternative is now `terminated(fold_parser,
      not(document::forbidden))`, so folding onto a forbidden line makes that alternative fail;
      `opt(...)` around it then gracefully stops the multi-line loop, leaving the marker line
      unconsumed for the closing quote to (correctly) never find, failing the scalar as
      unterminated rather than swallowing the marker as content.
- [x] **5f. Tests.** `directive.rs`: YAML directive (1.2 accepted, other minors accepted, major 2
      rejected), TAG directive (secondary/named/local-prefix forms), reserved directive (spec
      example 6.13's exact text, and a no-parameters case). `document.rs`: `explicit_document`
      same-line and empty-content cases; duplicate-`%YAML`/duplicate-TAG-handle error cases (the
      latter is spec example 6.15); full spec-example transcriptions via `yaml_stream` for 9.1
      (Document Prefix), 9.2 (Document Markers, two bare block sequences with no suffix between
      them), 9.4 (Explicit Documents, `...`-separated flow mappings), 9.5 (Directives Documents,
      `%TAG`-redefined primary handle applied to a block mapping), 9.6 (Streams, two
      `...`-terminated documents -- the multi-document end-to-end case), 6.13 (Reserved
      Directives), and 6.16 (Tag Shorthands, named + redefined-primary handles on a block
      sequence). Not every one of 6.14/6.17–6.22 got its own literal transcription (time-boxed);
      what they'd each add is already covered by the combination of `directive.rs`'s unit tests,
      `properties.rs`'s existing undeclared-handle test, and the above. Conformance report: 280/402
      (69.7%) → **369/402 (91.8%)**, the second-largest jump after Phase 4, with
      `UnexpectedSuccessOnErrorCase` back at 0 (it transiently went to 7 mid-phase from the two
      bugs described in 5d/5e above, both fixed before landing).

### Phase 6 -- Tag resolution / Core Schema

**Design decision (maintainer-approved 2026-07-05): resolution is *tag-only*, and the native
`Scalar::Null`/`Bool(bool)`/`Int(i64)`/`Float(f64)` variants have been deleted from
`value::Scalar` (along with the harness's placeholder match arms in
`tests/integration_tests.rs::scalar_value`).** `Scalar` is purely textual (the five style
variants, each `Cow<'i, str>`); `resolve()` only rewrites `Node::tag`
(`Tag::Unspecified`/`Tag::NonSpecific` → `Tag::Standard(...)`), never the scalar content.
**Construction phase (how natives are obtained later)**: typed accessors on `Node` interpreting
`(tag, text)` on demand (6e), and on top of those the Phase 8 serde `Deserialize` layer -- both
parse the retained lexeme against the caller's requested type at conversion time, so numeric
range/representation policy lives at the conversion site, not in the value model. Rationale:

- It matches the spec's own processing model: representation nodes carry *text* plus a tag;
  native data structures exist only in the Construct phase
  ([§3.1.2](https://yaml.org/spec/1.2.2/#312-construct)), which for this crate is the planned
  serde/accessor layer, not the parse output.
- Rewriting is lossy. Once `Plain("0x1A")` becomes `Int(26)`, the source lexeme is gone: the
  stream can't be re-resolved under a different schema (failsafe/JSON/core per §10.1–10.3, or a
  future custom schema -- resolution becomes a cheap re-runnable function of the retained text),
  and error messages can't quote what the user wrote (`~` vs `null`, `1e3` vs `1000.0`).
- The conformance harness *cannot* work with retyped scalars: `test.event` records scalar text
  verbatim, so `Plain("1e3")` → `Float(1000.0)` → `"1000"` would *introduce* `ContentMismatch`
  failures in 6d instead of clearing them. (The harness's `scalar_value` NOTE used to document
  this exact hazard; its four placeholder match arms were the only code touching the native
  variants, and were removed with them.)
- It removes redundant/illegal states: today `Tag::Standard(Bool)` + `Scalar::Int(3)` is
  representable. With tag-only resolution the tag is the single source of truth for kind and the
  text the single source of content.
- It dissolves open design question 3 (int overflow): an arbitrarily large `!!int` stays
  representable as tagged text; overflow becomes a conversion-time error surfaced by the accessor
  or serde layer against the integer type the *caller* asked for, which is the semantics serde
  users expect anyway.

- [ ] **6a. New `src/resolve.rs` post-pass** (post-pass keeps parsing schema-agnostic, consistent
      with the deferral comment in `plain.rs::plain`): `pub fn resolve(Stream) -> Result<Stream, ResolveError>`
      walking every node and rewriting *tags only* (see design note above; scalar content is
      untouched in every arm). `Tag::Unspecified` + `Scalar::Plain` → core-schema match (6b) →
      `Tag::Standard(Null|Bool|Int|Float)` or `Standard(Str)` on no match;
      `Unspecified` + any other scalar style (single/double/literal/folded) → `Standard(Str)`;
      `Unspecified` collections → `Standard(Map)`/`Standard(Seq)`;
      `Tag::NonSpecific` → str/map/seq by node kind. (The four native `Scalar` variants and the
      harness's placeholder match arms were already deleted when this design was decided.)
- [ ] **6b. Core-schema matchers** ([§10.3.2](https://yaml.org/spec/1.2.2/#1032-tag-resolution)),
      hand-written (no `regex` dependency), classifying text → `StandardTag` (not constructing
      values): null `null|Null|NULL|~|<empty>`; bool
      `true|True|TRUE|false|False|FALSE`; int `[-+]? [0-9]+`, `0o[0-7]+`, `0x[0-9a-fA-F]+`; float
      `[-+]? ( \. [0-9]+ | [0-9]+ ( \. [0-9]* )? ) ( [eE] [-+]? [0-9]+ )?`, `[-+]? \.(inf|Inf|INF)`,
      `\.(nan|NaN|NAN)`. (Under tag-only resolution these are pure classifiers; `i64` range is
      irrelevant here -- see 6e for where overflow policy now lives.)
- [ ] **6c. Explicit standard tags**: map `tag:yaml.org,2002:{str,null,bool,int,float,map,seq}`
      `Tag::Global` values to `Tag::Standard`, *validating* (via the 6b classifiers) that the
      content is well-formed for the forced tag and erroring when it isn't (e.g. `!!int foo`);
      content itself stays untouched.
- [ ] **6d. Harness**: call `resolve()` in `tests/integration_tests.rs` before the `ExpectedNode`
      comparison; this should clear most remaining `ContentMismatch` failures. (Text-vs-text
      comparison keeps working unchanged because resolution no longer rewrites content.)
- [ ] **6e. Construct-phase accessors**: `Node::as_str()` / `as_bool()` / `as_i64()` / `is_null()`
      etc., interpreting `(Tag::Standard(..), text)` on demand (e.g. `as_i64` parses decimal/octal/
      hex per the matched int form). This is where the old open-question-3 overflow policy lands:
      `as_i64` returns `None`/error on overflow while the node itself stays valid; a serde
      `Deserialize` impl (Phase 8) gets the same behavior per requested type for free.
- [ ] **6f. Tests**: the §10.3.2 resolution table and example 10.9 verbatim, plus accessor
      round-trips for each int form (`0o7`, `0x1A`, `-12`) and an `i64`-overflow case asserting
      the node survives and only the conversion fails.

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
      anchor *names* are not compared, because `value::Node` can't represent either yet -- comparison
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
      `cargo test`. **Current pass rate: 369/402 (91.8%)** -- Phase 2 (node properties/anchors)
      held flat at 129/402, since those mostly unlock cases that also need block collections
      (Phase 4) to reach content using them. Phase 3 (block scalars) nudged it to 132/402; block
      scalars themselves weren't reachable from real documents yet either (same Phase 4 dependency),
      but fixing a latent `chars::is_non_break` bug along the way (it excluded space, breaking any
      multi-word comment) unlocked a few flow-style cases with comments. Phase 4 (block
      collections) jumped it to 280/402 (69.7%), by far the largest single-phase gain -- block
      collections are most of real-world YAML, and landing them also finally exercised (and
      surfaced latent bugs in) the block-scalar and comment-handling machinery from earlier phases;
      see Phase 4's own writeup above for the three pre-existing bugs this uncovered
      (`spaces::line_comments`, `spaces::indent_less_than`, `detect_indentation`). **Phase 5
      (directives/explicit documents) jumped it to 369/402 (91.8%)**, the second-largest gain --
      most of the corpus' remaining `ParseErrorOnValidCase` failures were multi-document/directive
      streams that simply couldn't be reached before; landing it also surfaced (and fixed, see
      Phase 5's own writeup) two more pre-existing bugs, a missing `c-forbidden` guard in
      multi-line quoted scalars and a too-permissive `yaml_stream` dispatch arm, both caught because
      this harness flagged a transient `UnexpectedSuccessOnErrorCase` regression before they were
      fixed. Remaining failures are 21 `ParseErrorOnValidCase` (mostly Phase 6 tag/anchor-name
      corners: `!!`-tag validation, anchors containing `:`, tab-indented flow, empty flow
      collections, and one root-level zero-indented block-literal case unrelated to Phase 5), 5
      `StructuralMismatch` (flow-mapping edge cases with entirely-empty flow content, e.g.
      `{}`-shaped nodes), and 7 `ContentMismatch` (4 are the harness not yet surfacing
      `Tag::NonSpecific` as tag text `"!"`, or a `%XX`-escaped tag suffix not being unescaped for
      comparison -- both Phase 6 harness/tag-resolution concerns; 2 are double-quoted-scalar
      trailing-whitespace edge cases pre-dating this phase; 1 is a trailing-blank-lines-in-a-stream
      edge case). `ParserPanic` and `UnexpectedSuccessOnErrorCase` are both 0. Update this number
      whenever a phase lands. The real bugs this harness surfaced are now tracked as Phase 0 (and
      Phase 4, Phase 5) above.
- [ ] Once Phases 1-6 land, revisit `benches/benchmark.rs`'s commented-out plain-scalar lines.

### Phase 8 -- Polish (do last, or opportunistically)

- [ ] Clear the existing `cargo build` warnings (unused imports/params in `scalar.rs`, `plain.rs`,
      `block/map.rs`, `block/node.rs`) -- explicitly deferred until the phases above make real use of
      those parameters/types.
- [ ] `input.rs::WithLimit` is dead code: `key.rs` implements the 1024-char cap via
      `.with_taken()` + char counting instead. Pick one: switch `key.rs` to `WithLimit`, or delete
      `WithLimit`.
- [ ] Switch `spaces.rs::separate_lines`'s `alt` to `dispatch!` per its own TODO, for parity with
      `document.rs::yaml_stream`'s use of `dispatch!`.
- [ ] Public API ergonomics once the grammar is more complete: a top-level `ya::parse(&str) ->
      Result<value::Stream, _>` convenience function composing `yaml_stream` + `resolve()` (today
      callers must reach into `parse::yaml_stream` + `parse::input::Input` + pick a winnow `Error`
      type themselves, as seen in `tests/integration_tests.rs`).
- [ ] Consider `serde` integration (feature-gated) once the value model round-trips real documents.

### Open design questions (escalate to the maintainer)

Do not resolve these unilaterally; raise them when the phase that hits them starts.

1. **Alias eager substitution** (`alias.rs` clones the anchored `Node` per alias): this makes
   `ya` quadratic-to-exponential on alias-heavy input ("billion laughs"), and anchor *names* are
   not representable in `value::Node`, so presentation round-tripping is off the table. Options:
   keep (document as a non-goal, maybe add a size cap), switch to `Rc<Node>` sharing, or store
   anchor names on nodes. Affects Phase 2 wiring; the current plan assumes *keep as is*.
2. **Scalar style variants in `value::Scalar`**: the plan adds `Plain` (1h), `Literal`/`Folded`
   (3e) and `Tag::NonSpecific` (2c). Alternative: one string variant + a separate `style` field.
   Confirm the variant approach before Phase 1 lands it.
3. **Core-schema int overflow** (6b): ~~`Scalar::Int(i64)` can't hold arbitrary YAML ints. Fall
   back to `Float`? Keep as `Str`? Error? Add `u64`/`i128` variants? Pick before Phase 6.~~
   *Resolved by the Phase 6 tag-only-resolution decision (maintainer-approved 2026-07-05)*: since
   `resolve()` never rewrites scalar content, arbitrarily large ints remain representable as
   `Tag::Standard(Int)` + text, and overflow becomes a per-conversion error in the 6e accessors /
   Phase 8 serde layer against whatever integer type the caller requests.
