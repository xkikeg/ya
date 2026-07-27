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

Parse a YAML document into a tag-resolved `value::Stream` and read it with the typed accessors:

```rust
let stream = ya::parse("key: value\n").unwrap();
let doc = stream.documents()[0].as_node();
let ya::value::Content::Map(map) = &doc.value else {
    panic!("expected a mapping");
};
assert_eq!(map.entries()[0].value.as_str(), Some("value"));
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
    .unwrap()
    .into_iter()
    .collect::<ya::Result<_>>()
    .unwrap();
assert_eq!(points, vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
