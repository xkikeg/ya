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
- Plain scalars (`plain.rs`), one-line and multi-line, including line folding, the `#`-lookbehind
  and trailing-`:` rules of `ns-plain-char`, and the `c-forbidden` exclusion so a plain scalar can't
  swallow a `---`/`...` marker line (`document.rs::forbidden`). Produces `value::Scalar::Plain`, a
  new variant separate from `SingleStr`/`DoubleStr` since only plain-style scalars are eligible for
  core-schema resolution (Phase 6). See Phase 1 below (now complete).

Not implemented (stub returns `fail`, or missing outright) -- these are exactly the blockers a
`cargo build` warning-free pass would still leave semantically incomplete:
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
  (Phase 4) have the same cycle shape (`block_node` → `block_in_block` → `block_sequence` →
  `block_indented` → `block_node`) and need at least one such closure in the loop.
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

### Phase 2 -- Node properties (anchors & tags)

- [ ] **2a. Character classes in `chars.rs`**: `is_word_char`
      ([`ns-word-char`](https://yaml.org/spec/1.2.2/#rule-ns-word-char): alnum + `-`),
      `is_tag_char` ([`ns-tag-char`](https://yaml.org/spec/1.2.2/#rule-ns-tag-char)), and a
      `uri_chars` slice parser for [`ns-uri-char`](https://yaml.org/spec/1.2.2/#rule-ns-uri-char) --
      URI chars include `%xx` hex escapes, so a plain predicate isn't enough; use
      `repeat(1.., alt((one_of(<plain uri chars>).void(), ('%', hexdig, hexdig).void()))).take()`.
      Keep escapes *raw* (don't percent-decode) at parse time; decoding is a resolution concern.
- [ ] **2b. New `parse/properties.rs`.** Output types:
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
- [ ] **2c. Tag-handle → prefix map in parse state.** `Input`'s state is currently `AnchorStore`
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
- [ ] **2d. Wire into `flow/node.rs`** (replace the three `// TODO: fixme Support properties.`):
      - `flow_yaml_node` ([rule](https://yaml.org/spec/1.2.2/#rule-ns-flow-yaml-node)): third alt arm
        `(properties(context, n), opt(preceded(separate, flow_yaml_content)))` -- **properties with
        no content is legal** (`!!str &a` alone) and yields `Content::Empty` with that tag/anchor.
      - `flow_json_node`: `(opt((properties, separate)), flow_json_content)`.
      - `flow_node`: `alias_node | flow_content | (properties, opt(preceded(separate, flow_content)))`.
      - **Anchor registration**: after the full `Node` is built, if an anchor was present, register
        `input.anchor_store_mut().put(name.to_string(), node.clone())`. This needs `input` access,
        so write the property-carrying arms as hand-rolled closures (pattern: `key.rs:27`,
        `single.rs:49`), not pure combinator chains.
- [ ] **2e. Property slots in `block/scalar.rs` and Phase 4's `block_in_block`** are marked in those
      phases; if this phase lands first, nothing else to do here -- if a block phase lands first,
      leave `// TODO(Phase 2)` markers there.
- [ ] **2f. Tests.** Spec examples 6.23 (properties), 6.24/6.25 (verbatim, incl. the *invalid* one),
      6.26/6.27 (shorthands, incl. invalid), 6.28 (non-specific), 6.29 (anchors); end-to-end:
      `[&a foo, *a]` parses to two `foo` scalars.

### Phase 3 -- Block scalars (literal `|` / folded `>`)

- [ ] **3a. `block/header.rs`: `indentation_indicator` + `block_header`.**
      [`c-indentation-indicator`](https://yaml.org/spec/1.2.2/#rule-c-indentation-indicator):
      `opt(one_of('1'..='9')).map(|c| c.map(|c| c as usize - '0' as usize))`.
      [`c-b-block-header`](https://yaml.org/spec/1.2.2/#rule-c-b-block-header) allows the two
      indicators *in either order*, then `s-b-comment`: simplest faithful shape is
      `(opt(ind), chomping_indicator, opt(ind))` + verify not both `Some` (or `alt` of the two
      orders; note `chomping_indicator` never fails, it's `opt`-based). Return
      `(Option<usize>, ChompingMode)`; end with `spaces::space_break_comment`.
- [ ] **3b. Auto-detected indentation** ([§8.1.1.2](https://yaml.org/spec/1.2.2/#rule-c-indentation-indicator),
      the hard part of this phase). When the indicator is absent, `m` = (leading spaces of the first
      non-empty content line) − n, minimum 1; **error** if any leading *empty* line is more indented
      than that first non-empty line. Implement as a hand-rolled `detect_indentation(n)` that
      `checkpoint()`s, scans forward line-by-line counting leading spaces (empty = only spaces then
      break), computes `m`, `reset()`s, and returns the `IndentLevel` -- the actual content parse
      then re-consumes normally. Test with spec examples 8.2 and 8.3 (8.3 shows the invalid cases).
- [ ] **3c. New `block/literal.rs`.** Rules, 1:1:
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
- [ ] **3d. New `block/folded.rs`.** The most intricate rule cluster; transcribe each of
      `s-nb-folded-text` / `l-nb-folded-lines` / `s-nb-spaced-text` / `b-l-spaced` /
      `l-nb-spaced-lines` / `l-nb-same-lines` / `l-nb-diff-lines` /
      [`l-folded-content`](https://yaml.org/spec/1.2.2/#rule-l-folded-content) 1:1. Key semantics:
      breaks between same-indented text lines fold to a space (reuse
      `spaces::break_line_folded(BlockIn, ...)`), but "more-indented" lines (spaced text, starting
      with extra white) are kept literal with real breaks. Test each sub-rule against spec examples
      8.10–8.13.
- [ ] **3e. `block/scalar.rs::block_scalar`**: per
      [`s-l+block-scalar`](https://yaml.org/spec/1.2.2/#rule-s-l+block-scalar):
      `preceded(separate(context, n+1), (opt((properties(n+1), separate)) /* Phase 2 */, alt((literal(n), folded(n)))))`.
      Add `value::Scalar::Literal(Cow)` and `Scalar::Folded(Cow)` variants (block scalars always
      resolve to `str` in Phase 6, like quoted); update the harness conversion in
      `tests/integration_tests.rs`.
- [ ] **3f.** `block_scalar` is unreachable until Phase 4's `block_in_block` dispatches to it; until
      then, unit-test it directly.
- [ ] **3g. Tests.** [§8.1](https://yaml.org/spec/1.2.2/#81-block-scalar-styles) examples 8.1–8.13,
      especially the chomping matrix (8.4–8.6, incl. empty scalars) and folded specials (8.10–8.13).

### Phase 4 -- Block collections (biggest conformance jump; needs Phases 1 & 3, property slot from 2)

- [ ] **4a. Prerequisite signature fix.** `block_seq_entry` / `block_sequence` / `block_indented`
      return `Content` / `Vec<Content>` today (`block/seq.rs:25,47`, `block/node.rs:84`), but
      sequence items are `Node`s (`value::Content::Seq(Vec<Node>)`) and entries can carry
      properties/anchors. Change all three to produce `Node<'i>` / `Vec<Node<'i>>` (wrap
      property-less compact collections with `Node::unspecified`).
- [ ] **4b.** Land the Phase 0 `block/seq.rs` inverted-lookahead fix and the `key.rs` `opt()` fix
      here at the latest.
- [ ] **4c. `block/map.rs::block_map_entry`**
      ([`ns-l-block-map-entry`](https://yaml.org/spec/1.2.2/#rule-ns-l-block-map-entry)):
      `alt((block_map_explicit_entry(n), block_map_implicit_entry(n)))`, with:
      - `block_map_explicit_entry(n)`: `(block_map_explicit_key(n), alt((block_map_explicit_value(n), e_node)))`;
        key = `preceded('?', block_indented(BlockOut, n))` (bare `?` works because
        `block_indented`'s e-node arm matches empty); value = `preceded((spaces::indent(n), ':'), block_indented(BlockOut, n))`.
      - `block_map_implicit_entry(n)`: `(alt((block_map_implicit_key, e_node)), block_map_implicit_value(n))`;
        implicit key (`ns-s-block-map-implicit-key`) =
        `alt((key::implicit_json_key(BlockKey), key::implicit_yaml_key(BlockKey)))` -- these already
        exist (`key.rs:19,53`) including the 1024-char cap and the (post-4b optional) trailing
        in-line separation.
      - `block_map_implicit_value(n)` (`c-l-block-map-implicit-value`):
        `preceded(':', alt((block_node(BlockOut, n), terminated(e_node, spaces::line_comments))))`.
      - `e_node` helper: `empty.value(Node::unspecified(Content::Empty))` (pattern at
        `flow/map.rs:119`).
      - Reassurance for a scary-looking case: `key:value` (no space) correctly parses as *one plain
        scalar*, not a map -- that's real YAML 1.2 behavior, the `:`+plain-safe arm of
        `ns-plain-char` handles it.
- [ ] **4d. `block/node.rs::block_indented`**
      ([`s-l+block-indented(n,c)`](https://yaml.org/spec/1.2.2/#rule-s-l+block-indented)):
      `alt((compact_arm, block_node(context, n), (e_node, line_comments)))` where `compact_arm`
      consumes `s-indent(m)` for arbitrary `m ≥ 0` (`take_while(0.., ' ')`, `m` = consumed length;
      `alt` backtracks it if the compact parse fails) then
      `alt((compact_sequence(n'), compact_mapping(n')))` at `n' = indent_level + (m + 1)`
      (`IndentLevel: Add<usize>` exists, `spaces.rs:54`; spec's `n+1+m`).
      **This is the recursion cycle** (`block_indented` → `block_node` → ... → `block_indented`):
      write at least this function as a hand-rolled closure constructing children lazily (see the
      convention bullet above), or the parser value cannot be built.
- [ ] **4e. `ns-l-compact-sequence(n)` / `ns-l-compact-mapping(n)`** (new fns in `block/seq.rs` /
      `block/map.rs`): `(entry(n), repeat(0.., preceded(spaces::indent(n), entry(n))))` → wrap as
      `Content::Seq` / `Content::Map`.
- [ ] **4f. `block/node.rs::block_in_block`**
      ([`s-l+block-in-block`](https://yaml.org/spec/1.2.2/#rule-s-l+block-in-block)):
      `alt((block_scalar_arm, block_collection(context, n)))`. The scalar arm wraps Phase 3's
      `block_scalar` into a `Node`.
      `block_collection` ([`s-l+block-collection(n,c)`](https://yaml.org/spec/1.2.2/#rule-s-l+block-collection)),
      transcribed exactly:
      `(opt(preceded(separate(context, n+1), properties(n+1))) /* Phase 2 */, spaces::line_comments, alt((seq_space_arm, block_mapping(n))))`.
      The `seq-space(n,c)` dispatch is context-dependent: BLOCK-OUT → `block_sequence(n.prev())`
      (`IndentLevel::prev`, `spaces.rs:49`; the §8.2.2 "sequence under a mapping key may be at the
      same indentation" rule), BLOCK-IN → `block_sequence(n)`. Model it the way every other
      `c`-parameterized rule is modeled: a method on the `InOutBlock` trait (`context.rs:56`),
      `#[doc(alias = "seq-space")]`, implemented by `BlockOut`/`BlockIn`.
- [ ] **4g. Tests.** Spec [§8.2](https://yaml.org/spec/1.2.2/#82-block-collection-styles) examples
      8.14–8.22 (sequences, entry variants, mappings, explicit/implicit entries, compact 8.19,
      in-seq 8.20, in-block 8.22); end-to-end `document.rs` tests for `key: value\n` and
      `- a\n- b\n`. Update the Phase 7 pass rate -- this phase should move it the most.

### Phase 5 -- Directives & explicit/full documents

- [ ] **5a. New `parse/directive.rs`**:
      - `directive` ([`l-directive`](https://yaml.org/spec/1.2.2/#rule-l-directive)):
        `delimited('%', alt((yaml_directive, tag_directive, reserved_directive)), spaces::line_comments)`.
      - `yaml_directive` (`ns-yaml-directive`) + `yaml_version` (`ns-yaml-version`:
        `digit+ '.' digit+` → `(u32, u32)`). Behavior: hard error if major ≠ 1; accept minor ≠ 2
        silently, treating it as 1.2 (spec says "should issue a warning" but there is no warning
        channel -- note this in the doc comment). Duplicate `%YAML` in one document = error.
      - `tag_directive` (`ns-tag-directive`): `"TAG"`, separation, `properties.rs::tag_handle`,
        separation, `tag_prefix` (`ns-tag-prefix` = local `!...` or global URI form); inserts into
        the Phase 2c `TagHandles` via input state. Duplicate handle in one document = error.
      - `reserved_directive` (`ns-reserved-directive`): name + parameters, consumed and ignored.
- [ ] **5b. `document.rs::explicit_document`**
      ([`l-explicit-document`](https://yaml.org/spec/1.2.2/#rule-l-explicit-document)): add a
      `directives_end` rule fn (`c-directives-end` = `"---"`), then
      `alt((bare_document, terminated(e_node_document, spaces::line_comments)))` -- an explicit
      document may be empty (`Content::Empty`). Same-line content (`--- foo`) already works through
      `bare_document` → `flow_in_block`'s leading `separate`; `---\nfoo` works via the
      comment/line-break path of `separate_lines`.
- [ ] **5c. `document.rs::directive_document`** (`l-directive-document`):
      `preceded(repeat(1.., directive).map(|()| ()), explicit_document)`.
- [ ] **5d. Re-check `yaml_stream`'s dispatch** (`document.rs:32-43`): the `'-'` arm now reaches a
      real `explicit_document`; add a `'%'` arm → `directive_document.map(Some)` (only
      `any_document`'s alt covers directives today, and only for the first document).
- [ ] **5e. Per-document state reset.** Anchors are document-scoped
      ([§3.2.2.2](https://yaml.org/spec/1.2.2/#3222-anchors-and-aliases)) and so is the `%TAG` map:
      add `clear()` to `AnchorStore`/`TagHandles` and call them at each document boundary in
      `yaml_stream`'s loop. Also do Phase 1g (`c-forbidden` in multi-line plains) now if it was
      deferred -- multi-document conformance cases will fail without it.
- [ ] **5f. Tests.** Spec §6.8 examples 6.13–6.22 (directives) and §9.1/9.2 examples 9.1–9.6
      (documents/streams); a multi-document end-to-end test (`---` + `...` + directives).

### Phase 6 -- Tag resolution / Core Schema

- [ ] **6a. New `src/resolve.rs` post-pass** (post-pass keeps parsing schema-agnostic, consistent
      with the deferral comment in `plain.rs::plain`): `pub fn resolve(Stream) -> Result<Stream, ResolveError>`
      walking every node. `Tag::Unspecified` + `Scalar::Plain` → core-schema match (6b), rewriting
      the scalar to `Null`/`Bool(_)`/`Int(_)`/`Float(_)` and the tag to `Tag::Standard(...)`;
      `Unspecified` + any other scalar style (single/double/literal/folded) → `Standard(Str)`,
      content untouched; `Unspecified` collections → `Standard(Map)`/`Standard(Seq)`;
      `Tag::NonSpecific` → str/map/seq by node kind.
- [ ] **6b. Core-schema matchers** ([§10.3.2](https://yaml.org/spec/1.2.2/#1032-tag-resolution)),
      hand-written (no `regex` dependency): null `null|Null|NULL|~|<empty>`; bool
      `true|True|TRUE|false|False|FALSE`; int `[-+]? [0-9]+`, `0o[0-7]+`, `0x[0-9a-fA-F]+`; float
      `[-+]? ( \. [0-9]+ | [0-9]+ ( \. [0-9]* )? ) ( [eE] [-+]? [0-9]+ )?`, `[-+]? \.(inf|Inf|INF)`,
      `\.(nan|NaN|NAN)`. Int overflow past `i64` is an open policy question (see below) --
      **escalate before picking**.
- [ ] **6c. Explicit standard tags**: map `tag:yaml.org,2002:{str,null,bool,int,float,map,seq}`
      `Tag::Global` values to `Tag::Standard` + forced reinterpretation of the scalar content,
      erroring when content doesn't match the forced tag (e.g. `!!int foo`).
- [ ] **6d. Harness**: call `resolve()` in `tests/integration_tests.rs` before the `ExpectedNode`
      comparison; this should clear most remaining `ContentMismatch` failures.
- [ ] **6e. Tests**: the §10.3.2 resolution table and example 10.9 verbatim.

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
      `cargo test`. **Current pass rate: ~129/402 (32%)** -- update this number whenever a phase
      lands. The real bugs this harness surfaced are now tracked as Phase 0 above.
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
3. **Core-schema int overflow** (6b): `Scalar::Int(i64)` can't hold arbitrary YAML ints. Fall back
   to `Float`? Keep as `Str`? Error? Add `u64`/`i128` variants? Pick before Phase 6.
