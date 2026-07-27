# ya

[![Crates.io](https://img.shields.io/crates/v/ya.svg)](https://crates.io/crates/ya)
[![docs.rs](https://img.shields.io/docsrs/ya)](https://docs.rs/ya)
[![CI](https://github.com/xkikeg/ya/actions/workflows/ci.yml/badge.svg)](https://github.com/xkikeg/ya/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/ya.svg)](LICENSE)

"ya" (Yet Another YAML parser) is a pure-Rust implementation of the
[YAML 1.2.2 specification](https://yaml.org/spec/1.2.2/), built on top of the
[`winnow`](https://docs.rs/winnow) parser-combinator crate.

The defining design goal is to be **as naive as possible**: every production rule in the spec
grammar (`ns-plain`, `c-flow-mapping`, `s-l+block-node`, ...) has a corresponding Rust function,
named after the rule and linked to its exact spec anchor, composed the same way the spec composes
it -- no fused/optimized rewrites, no clever shortcuts. The spec is treated as the source of
truth, not prior art or idiomatic-Rust taste. See [`AGENT.md`](AGENT.md) for the full rationale
and module-by-module layout, if you're extending the crate.

## Status

100% conformant against the official
[yaml-test-suite](https://github.com/yaml/yaml-test-suite) (pinned to tag `data-2022-01-17`),
checked on every CI run. See [`AGENT.md`](AGENT.md) for the detailed phase-by-phase
implementation history and the current list of open items.

## Basic usage

Parse a YAML document into a tag-resolved `value::Document` and read it with the typed accessors:

```rust
let doc = ya::parse_document("key: value\n").unwrap();
let ya::value::Content::Map(map) = &doc.as_node().value else {
    panic!("expected a mapping");
};
assert_eq!(map.entries()[0].value.as_str(), Some("value"));
```

`ya::parse_stream` handles a `---`-separated stream instead, parsing one document per iteration
rather than all of them up front:

```rust
let values: ya::Result<Vec<_>> = ya::parse_stream("1\n---\n2\n").collect();
assert_eq!(values.unwrap().len(), 2);
```

With the optional `serde` feature enabled, deserialize directly into your own types:

```rust
#[derive(serde::Deserialize, Debug, PartialEq)]
struct Point {
    x: i64,
    y: i64,
}

let point: Point = ya::from_str("x: 1\ny: 2\n").unwrap();
assert_eq!(point, Point { x: 1, y: 2 });
```

```toml
[dependencies]
ya = { version = "0.4", features = ["serde"] }
```

For a `---`-separated multi-document stream, iterate one value per document:

```rust
let points: Vec<Point> = ya::Deserializer::from_str("x: 1\ny: 2\n---\nx: 3\ny: 4\n")
    .into_iter()
    .collect::<ya::Result<_>>()
    .unwrap();
assert_eq!(points, vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
```

## Errors

Every parsed node records the byte range it came from, so errors point at the text that caused
them and render it through [`annotate-snippets`](https://docs.rs/annotate-snippets). This holds for
syntax errors, Core Schema tag-resolution failures, and `Deserialize` failures alike:

```rust
let err = ya::parse_document("a: 1\nb: !!int nope\n").unwrap_err();
assert_eq!(
    err.to_string(),
    "\
error: explicit tag Int does not match scalar content \"nope\"
  |
2 | b: !!int nope
  |    ^^^^^^^^^^",
);
```

```text
error: invalid type: string "nope", expected i64
  |
2 | y: nope
  |    ^^^^
```

`ya::Error` is `'static`, so it propagates into `anyhow::Error`, `Box<dyn std::error::Error>` or
your own error enum with `?`. The position itself is available programmatically too, via
`ya::Span` on the node and `ya::Excerpt` on the error.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
