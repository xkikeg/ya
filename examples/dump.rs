//! Reads a YAML stream on stdin and prints what `ya` made of it.
//!
//! By default it dumps the parsed [`ya::value::Node`] of every document. With `--serde` (which
//! needs the crate's optional `serde` feature) it instead deserializes each document into the
//! `Config` demo type defined below, which is shaped to exercise every construct
//! `ya::de` supports.
//!
//! ```sh
//! printf 'a: 1\n---\nb: [x, y]\n' | cargo run --example dump
//! cargo run --example dump --features serde -- --help
//! ```
//!
//! Argument parsing is hand-rolled on purpose: `ya` is deliberately zero-dependency beyond
//! `winnow`, and one example is not worth pulling in an argument-parsing crate.

use std::io::Read as _;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: dump [OPTIONS] < input.yaml

Reads a YAML stream on stdin and prints the parsed representation.

Options:
      --document  Parse the input as a single document instead of a stream.
      --serde     Deserialize each document into this example's demo type and
                  print that instead of the node dump. Requires the crate's
                  `serde` feature (cargo run --example dump --features serde).
  -h, --help      Print this help, including the demo type's schema.
";

/// A document matching the `--serde` demo type, shown by `--help` so the flag is usable without
/// shipping a separate fixture file.
const SAMPLE: &str = "\
name: demo
description: an example config
version: 3
enabled: true
ratio: 0.75
note: ~
tags: [a, b]
limits:
  cpu: 2
  memory: 4096
env:
  RUST_LOG: debug
steps:
  - noop
  - run: echo hi
  - copy: [src, dst]
  - wait: {seconds: 3}
";

/// Exit code for a failed parse / resolve / deserialization.
const FAILURE: ExitCode = ExitCode::FAILURE;

/// Exit code for a misuse of the command line itself. A function rather than a `const` because
/// `ExitCode::from` isn't `const fn`.
fn usage_error() -> ExitCode {
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let options = match Options::from_args(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        // `--help`: already printed, nothing left to do.
        Ok(None) => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("dump: {message}\n\n{USAGE}");
            return usage_error();
        }
    };

    let input = match read_stdin() {
        Ok(input) => input,
        Err(e) => {
            eprintln!("dump: cannot read stdin: {e}");
            return FAILURE;
        }
    };

    if options.serde {
        return run_serde(&input, options.document);
    }
    if options.document {
        dump_document(&input)
    } else {
        dump_stream(&input)
    }
}

/// The flags `dump` understands.
#[derive(Debug, Default)]
struct Options {
    /// Parse a single document rather than a stream.
    document: bool,
    /// Deserialize into the demo type instead of dumping nodes.
    serde: bool,
}

impl Options {
    /// Parses the command line, returning `Ok(None)` when `--help` was asked for (already
    /// printed) and `Err(message)` on a usage error.
    fn from_args(args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut options = Self::default();
        for arg in args {
            match arg.as_str() {
                "--help" | "-h" => {
                    print!("{USAGE}\n{}", help_epilogue());
                    return Ok(None);
                }
                "--document" => options.document = true,
                "--serde" => options.serde = true,
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Some(options))
    }
}

fn read_stdin() -> std::io::Result<String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

/// Prints every document's root node, keeping going after a resolve error the way
/// [`ya::parse_stream`] itself does (only a *parse* error ends the stream).
fn dump_stream(input: &str) -> ExitCode {
    let mut code = ExitCode::SUCCESS;
    for (i, document) in ya::parse_stream(input).enumerate() {
        match document {
            Ok(document) => println!("# document {i}\n{:#?}", document.as_node()),
            Err(e) => {
                eprintln!("dump: document {i}: {e}");
                code = FAILURE;
            }
        }
    }
    code
}

/// Prints the root node of a single-document stream.
fn dump_document(input: &str) -> ExitCode {
    match ya::parse_document(input) {
        Ok(document) => {
            println!("{:#?}", document.as_node());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dump: {e}");
            FAILURE
        }
    }
}

#[cfg(feature = "serde")]
mod demo {
    //! The `--serde` demo type: one coherent "build config" shaped to cover every construct
    //! `ya::de` implements -- the scalar types the Core Schema resolves, sequences, mappings with
    //! dynamic keys, nested structs, `Option`, and all four serde enum variant kinds.

    // Every field here is read, but only by the derived `Debug` impl, which rustc's dead-code
    // analysis deliberately ignores. Printing them *is* what this example does with them.
    #![allow(dead_code)]

    use std::borrow::Cow;
    use std::collections::BTreeMap;

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Config<'a> {
        /// A plain `!!str`.
        pub name: String,
        /// Borrowed straight out of the input when the scalar needs no unescaping or line
        /// folding, and allocated only when it does -- which is what `ya`'s `Cow`-based scalars
        /// are for. (`Cow`'s own `Debug` is transparent, so which one happened doesn't show in
        /// the dump; `matches!(config.description, Cow::Borrowed(_))` is how you'd check.) A
        /// bare `&'a str` field would show off the same zero-copy path but reject any scalar the
        /// parser had to allocate for, such as a double-quoted `"an\texample"`.
        #[serde(borrow)]
        pub description: Cow<'a, str>,
        /// `!!int`, narrowed to the type asked for here rather than at parse time.
        pub version: u32,
        /// `!!bool`.
        pub enabled: bool,
        /// `!!float`.
        pub ratio: f64,
        /// An explicit `null` (`~`) or a missing key both give `None`.
        #[serde(default)]
        pub note: Option<String>,
        /// `!!seq`.
        pub tags: Vec<String>,
        /// A nested mapping with a fixed shape.
        pub limits: Limits,
        /// A `!!map` whose keys aren't known ahead of time.
        pub env: BTreeMap<String, String>,
        pub steps: Vec<Step>,
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Limits {
        pub cpu: u32,
        pub memory: u64,
    }

    /// Externally tagged, the representation `ya::de` supports: a bare scalar names a unit
    /// variant, and a single-entry mapping `{variant: value}` carries any of the others.
    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Step {
        /// `- noop`
        Noop,
        /// `- run: echo hi`
        Run(String),
        /// `- copy: [src, dst]`
        Copy(String, String),
        /// `- wait: {seconds: 3}`
        Wait { seconds: u32 },
    }
}

#[cfg(feature = "serde")]
fn run_serde(input: &str, document: bool) -> ExitCode {
    use demo::Config;

    if document {
        return match ya::from_str::<Config>(input) {
            Ok(config) => {
                println!("{config:#?}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("dump: {e}");
                FAILURE
            }
        };
    }

    let mut code = ExitCode::SUCCESS;
    for (i, config) in ya::Deserializer::from_str(input)
        .into_iter::<Config>()
        .enumerate()
    {
        match config {
            Ok(config) => println!("# document {i}\n{config:#?}"),
            Err(e) => {
                eprintln!("dump: document {i}: {e}");
                code = FAILURE;
            }
        }
    }
    code
}

#[cfg(not(feature = "serde"))]
fn run_serde(_input: &str, _document: bool) -> ExitCode {
    eprintln!(
        "dump: --serde needs the crate's `serde` feature, which this example was built without\n\
         \x20     rebuild with: cargo run --example dump --features serde -- --serde"
    );
    usage_error()
}

#[cfg(feature = "serde")]
fn help_epilogue() -> String {
    format!("`--serde` deserializes each document into:\n\n{SCHEMA}\nSample input:\n\n{SAMPLE}")
}

#[cfg(not(feature = "serde"))]
fn help_epilogue() -> String {
    format!(
        "`--serde` is unavailable: this example was built without the crate's `serde` feature.\n\
         Rebuild with `cargo run --example dump --features serde` to deserialize into:\n\n\
         {SCHEMA}\nSample input:\n\n{SAMPLE}"
    )
}

/// The demo type's shape, spelled out for `--help` so it's readable without the source at hand.
const SCHEMA: &str = "\
struct Config {
    name: String,
    description: Cow<str>,
    version: u32,
    enabled: bool,
    ratio: f64,
    note: Option<String>,
    tags: Vec<String>,
    limits: struct { cpu: u32, memory: u64 },
    env: BTreeMap<String, String>,
    steps: Vec<enum { noop, run(String), copy(String, String), wait { seconds: u32 } }>,
}
";
