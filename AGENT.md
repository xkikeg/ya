# AGENT.md

Guidance for agents (and humans) working on `ya`, a pure-Rust YAML 1.2.2 parser.

Everything here describes the code as it stands. The phase-by-phase record of how it got built --
deviations from the original plan, root causes of the bugs found along the way, and the approaches
that were tried and abandoned -- lives in [`docs/agents/history.md`](docs/agents/history.md).

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
  lib.rs             re-exports `parse`, `resolve`, `value`, the error types and the two
                        top-level entry points (`parse_document` / `parse_stream`)
  value.rs            output data model (Stream, Document, Node, Content, Scalar, Mapping, Tag,
                        Span) + Construct-phase accessors
                        (Node::is_null/as_bool/as_str/as_i64/as_f64)
  error.rs            `Error` / `Result` / `ParseError<'i>` / `OwnedParseError` / `Excerpt`
                        (the located source an error points at, rendered via annotate-snippets)
  documents.rs        lazy, document-at-a-time parsing: `Documents` iterator + `parse_stream` /
                        `parse_document`, driving `parse::document`'s stream_head/stream_step
  de.rs (serde only)  `Deserialize` support: `from_str`/`from_bytes`, `Deserializer`,
                        `StreamDeserializer` (lazy, over `Documents`), `NodeDeserializer`
  resolve.rs          Core Schema tag resolution post-pass (`resolve(Stream) -> Result<Stream, _>`,
                        `resolve_document(Document) -> Result<Document, _>`)
  parse.rs            module root, re-exports `yaml_stream`/`yaml_document` as the parser entry
                        points
  parse/
    error.rs          ParserError trait alias (winnow error bounds used throughout)
    input.rs           InputStream trait alias + `Input` (LocatingSlice + Stateful<AnchorStore>,
                        also `winnow::stream::Location` so nodes can be spanned)
    span.rs            `spanned()`: records the input range a node parser matched onto the node
                        it produced (not a spec production)
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
    properties.rs       c-ns-properties: anchors (&name) and the three tag forms, plus the shared
                        build_node helper (tag resolution + anchor registration)
    tag_handles.rs      tag handle -> prefix map (the two defaults, plus whatever %TAG registers)
    directive.rs        Chapter 6.8: %YAML / %TAG / reserved directives
    document.rs         Chapter 9: l-yaml-stream, l-any-document, l-bare-document,
                        l-explicit-document, l-directive-document, document prefix/suffix;
                        public `yaml_stream()`/`yaml_document()` entry points + its tests;
                        `stream_head`/`stream_step` (one loop iteration of l-yaml-stream, shared
                        by `yaml_stream` and by `crate::documents`' lazy iterator)
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
                          s-l+flow-in-block / s-l+block-indented
    testing.rs (test-only) parse-helper wrapping winnow's `.parse()` for use in unit tests
```

## Current state

The grammar is complete and the parser is **100% conformant (402/402)** against the official
yaml-test-suite, checked unskipped on every CI run. Every production named in the tree above is
implemented and reachable from `yaml_stream`: there is no `fail` stub or otherwise unimplemented
rule left anywhere in the parser, and both `ParserPanic` and `UnexpectedSuccessOnErrorCase` are 0 in
the conformance report. On top of the grammar sit Core Schema tag resolution (`resolve.rs`), typed
Construct-phase accessors on `Node`, a source span on every node with `annotate-snippets`-rendered
diagnostics, lazy document-at-a-time parsing (`documents.rs`), and optional `serde::Deserialize`
support (`de.rs`, behind the `serde` feature).

Deliberate non-goals -- these are decisions, not gaps, so don't "fix" them without raising it first:

- **No `Serialize`, and no presentation round-tripping.** `value::Node` models the spec's
  *representation* graph. Scalar style survives (`Plain`/`SingleStr`/`DoubleStr`/`Literal`/`Folded`)
  but flow-vs-block collection style, indentation, comments and anchor *names* do not, so `T -> Node`
  would have to invent presentation decisions this crate doesn't model at all. See `de.rs`'s module
  docs.
- **Resolution is tag-only.** `resolve()` rewrites `Node::tag` and never scalar content, so the
  source lexeme stays recoverable (re-resolution under another schema, quoting the original text in
  errors, event-level comparison against the corpus). Native values are a Construct-phase concern,
  produced on demand by the `Node` accessors and the serde layer, which parse the retained text
  against the type the *caller* asked for. See `resolve.rs` and `value::Scalar`'s doc comments.
- **`Tag::NonSpecific` (an explicitly written `!`) is deliberately left unresolved** rather than
  rewritten to str/map/seq, because the corpus' own ground truth records it as the literal tag `"!"`.
  Consumers that don't care treat it as equivalent to its forced kind (see `Node::as_str`).
- **Aliases are substituted eagerly**: `alias.rs` clones the anchored `Node` at each alias, which is
  quadratic-to-exponential on alias-heavy input ("billion laughs"). Currently accepted; see
  [Open items](#open-items).

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
  not-yet-implemented rule rather than panicking. (There are none left in the parser today.)
- `IndentLevel` internally stores spec `n + 1` (so the spec's `n = -1` "no indent yet" becomes `0`);
  use `IndentLevel::initial()` / `IndentLevel::new(n)` / `.get()` / `.prev()` / `+ usize` rather than
  constructing the wrapped integer directly.
- The crate depends on `winnow` 1.0 (with the `simd` feature) for parsing and `annotate-snippets`
  for rendering diagnostics, and nothing else -- **keep it minimal-dependency**: no
  `regex`, no `once_cell`, no `thiserror`; hand-write small matchers instead. Two direct
  dependencies is the budget, and a third needs a reason as good as those two. When bumping winnow,
  check for combinator signature changes first; `annotate-snippets` sets the crate's MSRV (1.85.0),
  so a bump there is an MSRV decision.
- **Recursive grammar rules must break the construction cycle with a hand-rolled closure.**
  Combinators like `preceded(a, b)` *store* their sub-parsers eagerly, so an eager cycle
  (`flow_node` → `flow_content` → `flow_sequence` → `flow_node`) would be an infinitely-sized value.
  The existing code breaks each cycle by constructing sub-parsers *inside* a
  `move |input: &mut Input| { ... child(...).parse_next(input) }` closure body (see
  `flow/map.rs::flow_map_entries`, `single.rs::non_break_single_multi_line`). Block collections
  turned out to have *two independent* cycles, not one -- see the history doc's Phase 4 writeup for
  the one that isn't visually obvious from the spec composition (`block_map_implicit_value` →
  `block_node` directly, never passing through `block_indented`'s own closure). **When adding a new
  recursive rule, trace the whole call graph for cycles, not just the one the grammar's shape
  suggests.**
- **Don't wrap the node parsers in new combinator layers; call the wrapper from inside a closure
  instead.** Same mechanism as the previous bullet, different symptom. The parser types here nest
  dozens deep, so adding a combinator (e.g. `.with_span().map(...)`) at *each* level multiplies the
  monomorphized type at every level below it: `spanned` first landed as an ordinary
  combinator and `cargo build` went from seconds to **18+ CPU-minutes without finishing**, while
  `cargo check` stayed at 2.7s -- type checking is fine, it's codegen that explodes, so a quick
  `cargo check` will not warn you. The fix is the closure form (`parse/span.rs::spanned` takes
  `&mut Input` plus the parser, and callers build that parser inside a
  `move |input: &mut Input| ...` body), which keeps the wrapped parser's type out of the enclosing
  function's `impl Parser` return type entirely. If a build suddenly takes minutes, this is the
  first thing to suspect.
- winnow 1.0 idioms already in use, for reference when transcribing new rules: `alt`, `dispatch!`,
  `repeat` (note: `repeat(0.., p).map(|()| ())` to pick the `()` accumulator), `opt`, `preceded` /
  `terminated` / `delimited`, `peek`, `not` (peeks; succeeds at EOF), `empty.value(x)` (for `e-node`),
  `fail`, `.take()` (borrow the matched slice), `.with_taken()`, `.flat_map()`, `.verify()`,
  `.void()`, `.value()`, `.map()`.

## Working the hard cases

**When the code, a corpus case, and your instinct disagree about what a rule should do, open the
spec section that rule's function links to and read the whole rule -- including the inline
annotations in the grammar block and the prose around it. That text is the requirements document.**
Nearly every hard bug in this parser's history was a rule whose spec text said something the
composition alone didn't show, and nearly every one was fixed by reading it rather than by
reasoning about the code. The recurring shapes, each with the case that taught it (full writeups in
[`docs/agents/history.md`](docs/agents/history.md)):

1. **A rule's restrictions may live in an inline annotation, not in its composition.**
   `c-l-block-seq-entry`'s `-` and `c-mapping-key`'s `?` are both annotated
   `# not followed by non-ws char`. Missing that made `-foo` / `?foo` parse as collection markers
   that swallowed the rest of the line, instead of as plain scalars. Both now guard with
   `not(one_of(chars::is_non_space))` (`block/seq.rs::block_seq_entry`,
   `block/map.rs::block_map_explicit_key`). **Live lead**: the spec puts the identical annotation on
   `l-block-map-explicit-value`'s and `c-l-block-map-implicit-value`'s `:`, and neither is guarded
   today -- no corpus case exercises it, so it was left alone.
2. **Some rules are defined in prose, not as a production.** A block scalar's auto-detected content
   indentation ([§8.1.1.1](https://yaml.org/spec/1.2.2/#8111-block-indentation-indicator)) is
   "the leading spaces of the first non-empty line" -- an *absolute* quantity. Re-deriving it as
   `n + m` looks equivalent and is wrong at the document root
   (`block/header.rs::detect_indentation`). Prefer the literal reading over the one that fits the
   surrounding code's shape.
3. **Compare `IndentLevel` values, never `.get()`.** `get()`'s `saturating_sub` collapses the
   document-root sentinel (spec `n = -1`) and `n = 0` into the same `0`, so `spaces <= n.get()`
   silently rejected legitimate zero-indented root content. Convert the candidate with
   `IndentLevel::new(spaces)` and compare the two `IndentLevel`s, so both sides go through the same
   encoding.
4. **In an `alt`, an arm that can succeed on a proper *prefix* must not go first.** The failure mode
   isn't a wrong parse, it's a *short* one: `ns-flow-yaml-node`'s "properties with no content" arm is
   legal on its own, so for `&a [a, &b b]: *b` it consumed just `&a`, returned `Ok`, and the
   JSON-key arm never ran. Fix by trying the mandatory-content arm first (`flow/map.rs`'s implicit
   entries try the JSON key before the YAML key, matching what block mappings already did). Same
   class: `ns-reserved-directive`'s `ns-char+` name also matches `YAML`/`TAG`, so a malformed
   `%YAML 2.0` backtracked into "ignored reserved directive" -- fixed by peeking the name and
   dispatching on an exact match (`directive.rs::directive_body`).
5. **`opt(...)` commits the moment its body succeeds.** A mandatory check placed *after* the `opt`
   can't un-commit what the `opt` consumed; the whole parser hard-fails instead of backtracking to
   the "absent" shape. Restructure into an `alt` whose every branch bundles the trailing check, so a
   downstream failure backtracks the entire branch (`block::node::block_collection`'s
   leading-properties arms).
6. **Only a sub-parse's caller can retry it more narrowly.** Once `properties()` returns `Ok` having
   greedily crossed a line break to grab the *next* node's anchor, no `alt`/`opt` above it can reach
   back in and ask for a smaller result. The fix is flat sibling arms (greedy combo, tag-only,
   anchor-only, none) in the *same* combinator that performs the trailing `s-l-comments` check.
7. **EOF usually has to behave like a line break.** `<end-of-input>` is defined as matching the empty
   string, but that is about *matching*, not about semantics: Clip/Keep still owe a final `\n` at
   EOF (`b-chomped-last`), `l-keep-empty` still has to count an unterminated final blank line, and
   `s-b-comment` already did this (`alt((line_break, eof))`). A source file that simply stops where
   a break would be is legal input.
8. **Anything that can consume across a line break needs `not(document::forbidden)`**, or it
   swallows a `---` / `...` marker as content. This was added three separate times -- plain scalars,
   then quoted scalars, then block literal/folded -- each time as a corpus regression. Assume the
   next multi-line rule needs it too.
9. **winnow: a `repeat` body must always consume.** Any parser that can succeed empty (`line_comment`
   and `separate_in_line` both can, at EOF or at start-of-line) trips a debug assert inside `repeat`.
   Guard with `.take().verify(|s: &&str| !s.is_empty())`, or hand-roll the loop and break on empty
   consumption. This has bitten at least three times, in unrelated modules.
10. **When the spec text is genuinely ambiguous, the yaml-test-suite fixture is the tiebreaker** --
    and note *what level* it speaks at. `test.event` records the **representation**: a tag appears
    only when one was explicitly written, never the implicitly-resolved one. That is what settled
    leaving `Tag::NonSpecific` unresolved, and it's why `tests/integration_tests.rs` compares shape +
    content + explicit tag rather than resolved tags.
11. **Tools, when reading isn't enough.** `cargo build --features winnow/debug` prints the exact
    parse trace (every `trace(...)` label) and is what root-caused the last remaining corpus failure.
    `target/yaml_conformance_report.txt` is rewritten by any `cargo test`. And the live spec HTML
    truncates before Chapter 10 when fetched -- read `spec/1.2.2/spec.md` in the
    [`yaml/yaml-spec`](https://github.com/yaml/yaml-spec) repo for the schema definitions.

Two more traps are documented as conventions above, since they're about how the code is built rather
than what the spec says: trace the *whole* call graph when adding a recursive rule, and remember that
`cargo check` will not warn you about a codegen explosion.

## Testing

- `cargo test` runs unit tests colocated in each module (`#[cfg(test)] mod tests`) plus
  `tests/integration_tests.rs`.
- `testdata/yaml-test-suite` is a git submodule (the official
  [yaml/yaml-test-suite](https://github.com/yaml/yaml-test-suite)); it must be checked out
  (`git submodule update --init`) for the integration test to find cases. It is pinned to tag
  `data-2022-01-17`.
- The integration test both checks "error cases fail to parse" and, for valid cases, structurally
  compares the parse against the suite's `test.event` fixture (representation-level: node shape,
  scalar content, explicitly-written tag -- not presentation style, anchor names, or the
  fully-resolved tag of an implicitly-tagged node; see `tags_match`'s doc comment there for why).
- **The current pass rate is 402/402 (100.0%)**, and it must stay there. Run
  `cargo test --test integration_tests conformance_report -- --nocapture` (or just read
  `target/yaml_conformance_report.txt` after any `cargo test`) for the breakdown by failure
  category.
- `benches/benchmark.rs` (Criterion) benchmarks `flow_sequence` over a mix of plain, single- and
  double-quoted scalar entries. `cargo bench --bench benchmark -- --test` runs it once, untimed, as
  a smoke test.
- `examples/dump.rs` is a hand-rolled-argv CLI over the *public* API (no `clap`, per the
  minimal-dependency rule): it reads YAML on stdin and dumps each document's `value::Node`
  (`--document` for a single document instead of a stream), or with `--serde` deserializes into a
  demo type covering every construct `de.rs` supports. `cargo run --example dump -- --help` prints
  that demo schema and a matching sample document. It deliberately has no `required-features`, so
  a no-feature build still compiles and `--serde` fails at *runtime* with a "rebuild with
  `--features serde`" message; the serde half is `#[cfg(feature = "serde")]`-gated inside the file.

## How to work an item

1. Read the linked spec section *in full* before writing code; the rule text (and its examples) is
   the requirements document. See [Working the hard cases](#working-the-hard-cases) for what tends
   to hide in it.
2. Follow the conventions above: spec-transliterated names, `#[doc(alias)]`, spec-anchor doc link,
   `trace("module::name", ...)`, and the generic `Context`/`Input`/`Error` signature shape.
3. Unit-test with the spec's own example strings, colocated `#[cfg(test)] mod tests`.
4. Run the full `cargo test`, then
   `cargo test --test integration_tests conformance_report -- --nocapture`; the pass count must not
   drop below the number recorded under [Testing](#testing), and if it goes *up*, update that
   number in the same change.
5. Also run `cargo clippy --all-targets --all-features -- -D warnings` and the same without
   `--all-features`; both are expected to be clean.
6. Stop and ask the maintainer when you hit one of the [open items](#open-items) below, or any other
   decision that changes the public API or the value model.
7. When a substantial piece of work lands, append its writeup -- what deviated from the plan and
   why, what root causes turned up -- to [`docs/agents/history.md`](docs/agents/history.md).

## Open items

Small and self-contained, in no particular order:

- **`spaces.rs::separate_lines` should use `dispatch!` instead of `alt`** (its own `// TODO` at
  `spaces.rs:175`), for parity with `document.rs::yaml_stream`. Deliberately deferred: the `alt` is
  correct, this is a perf/style nit that carries its own small regression risk for zero behaviour
  change.
- **The 1024-character implicit-key limit** (`key.rs`, two `// TODO`s): the limit is enforced, but
  *after the fact* -- `implicit_yaml_key`/`implicit_json_key` parse the whole key, then reject it if
  the taken text exceeded 1024 characters. Whether the parse itself should be bounded up front (so a
  pathological key costs bounded work rather than being parsed and thrown away) is the open
  question. Note the earlier `WithLimit` input wrapper written for exactly this was deleted as dead
  code, so a bounded version would start over.

Escalate rather than deciding unilaterally:

- **Alias eager substitution.** `alias.rs` clones the anchored `Node` per alias, making `ya`
  quadratic-to-exponential on alias-heavy input ("billion laughs"), and anchor *names* aren't
  representable in `value::Node`, so presentation round-tripping stays off the table. Options: keep
  it (document as a non-goal, possibly add a size cap), switch to `Rc<Node>` sharing, or store
  anchor names on nodes. The current position is *keep as is*.

Two earlier design questions -- scalar style variants in `value::Scalar`, and Core Schema integer
overflow -- are resolved; their reasoning is in
[`docs/agents/history.md`](docs/agents/history.md#resolved-design-questions).
