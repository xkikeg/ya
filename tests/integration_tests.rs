//! Runs `ya` against the official `yaml/yaml-test-suite` corpus (vendored as a submodule at
//! `testdata/yaml-test-suite`).
//!
//! For "error" cases (a case dir containing an `error` marker file) we only check that parsing
//! fails, matching the corpus convention. For valid cases we parse the corpus' `test.event`
//! fixture (libyaml-style event stream) into a tree ([`ExpectedNode`]) and compare it structurally
//! against the tree `ya` itself produces for `in.yaml`.
//!
//! The comparison is deliberately scoped to the *representation* (node kind, nesting, scalar
//! content, resolved tag), not *presentation* (plain vs. quoted style, flow vs. block style, or
//! anchor names): `value::Node` doesn't carry that information today (its stated purpose is
//! eventual serde/Construct-phase deserialization, not YAML round-tripping), and
//! `value::Scalar::SingleStr` currently represents both plain and single-quoted scalars (see
//! `src/parse/plain.rs`), so a style-exact comparison would produce false negatives unrelated to
//! real conformance gaps. Alias resolution is instead handled entirely on the oracle side: while
//! parsing `test.event`, `=ALI` events are expanded against a locally built anchor→subtree map,
//! mirroring `ya`'s own eager alias substitution in `src/parse/alias.rs`. So both sides end up as
//! alias-free trees, and comparison stays meaningful for anchor/alias cases even though `ya`
//! doesn't retain anchor names on its own [`ya::value::Node`] yet.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use rstest::rstest;
use winnow::{error::ContextError, Parser};
use ya::parse::yaml_stream;
use ya::value;

#[rstest]
fn check_test_suite(
    #[base_dir = "testdata/yaml-test-suite/"]
    #[files("**/in.yaml")]
    #[dirs]
    case: PathBuf,
) {
    if let Err(failure) = check_case(&case) {
        panic!("{}: {failure}", case.display());
    }
}

/// Walks the whole corpus and reports the overall pass rate, without ever failing itself, so a
/// plain `cargo test` always regenerates a fresh conformance snapshot. See
/// `target/yaml_conformance_report.txt` (also printed to stdout) for the full breakdown.
#[test]
fn conformance_report() {
    let cases = find_cases(Path::new("testdata/yaml-test-suite"));
    let total = cases.len();
    let mut passed = 0usize;
    let mut by_category: HashMap<FailureCategory, usize> = HashMap::new();
    let mut failures: Vec<(PathBuf, CaseFailure)> = Vec::new();

    // `ya`'s parser is early-stage enough that it can still panic (e.g. a tripped `winnow`
    // internal invariant) rather than cleanly returning a parse error. Catch that per-case so one
    // crashing input doesn't lose the whole report. The panic hook is replaced (rather than left
    // as the default, which would flood stderr with ~400 potential backtraces) with one that
    // stashes the formatted message -- `PanicHookInfo`'s `Display` impl includes location + the
    // panic message -- so it can be read back after `catch_unwind` reports an `Err`.
    // TODO: this catch_unwind/panic-hook dance is a workaround for real `ya` bugs (see AGENT.md's
    // Phase 7 notes on AVM7/HWV9); once conformance_report shows zero ParserPanic failures, rip it
    // out and let a panic fail the test loudly again, like it does for the plain per-case rstest.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        LAST_PANIC_MESSAGE.with(|cell| *cell.borrow_mut() = Some(info.to_string()));
    }));
    for case in &cases {
        let outcome = catch_unwind(AssertUnwindSafe(|| check_case(case)));
        let result = outcome.map_err(|_| {
            let message = LAST_PANIC_MESSAGE
                .with(|cell| cell.borrow_mut().take())
                .unwrap_or_else(|| "panicked without a captured message".to_string());
            CaseFailure {
                category: FailureCategory::ParserPanic,
                message,
            }
        });
        match result {
            Ok(Ok(())) => passed += 1,
            Ok(Err(failure)) | Err(failure) => {
                *by_category.entry(failure.category).or_insert(0) += 1;
                failures.push((case.clone(), failure));
            }
        }
    }
    std::panic::set_hook(previous_hook);

    let pct = if total == 0 {
        0.0
    } else {
        100.0 * passed as f64 / total as f64
    };

    let mut report = String::new();
    let _ = writeln!(report, "YAML conformance report (yaml-test-suite)");
    let _ = writeln!(report, "==========================================");
    let _ = writeln!(report, "{passed}/{total} cases passing ({pct:.1}%)");
    for category in FailureCategory::ALL {
        let count = by_category.get(&category).copied().unwrap_or(0);
        let _ = writeln!(report, "  {category:?}: {count}");
    }
    let _ = writeln!(report);
    let _ = writeln!(report, "Failures:");
    for (case, failure) in &failures {
        let case_dir = case.parent().unwrap();
        let label = std::fs::read_to_string(case_dir.join("==="))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let name = case_dir.file_name().unwrap().to_string_lossy();
        let _ = writeln!(
            report,
            "  [{:?}] {name} {label}: {}",
            failure.category, failure.message
        );
    }

    println!("{report}");
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = std::fs::create_dir_all(&target_dir);
    let _ = std::fs::write(target_dir.join("yaml_conformance_report.txt"), &report);
}

/// Recursively finds every `in.yaml` under `base` (case dirs may nest variants, e.g.
/// `SM9W/00/in.yaml`, `SM9W/01/in.yaml`).
fn find_cases(base: &Path) -> Vec<PathBuf> {
    let mut cases = Vec::new();
    collect_cases(base, &mut cases);
    cases.sort();
    cases
}

fn collect_cases(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs = Vec::new();
    let mut has_in_yaml = false;
    for entry in entries.flatten() {
        // yaml-test-suite ships a `name/<slug>` tree of symlinks aliasing real case dirs by
        // human-readable name; `DirEntry::file_type()` (unlike `Path::is_dir()`) doesn't follow
        // symlinks, so skipping non-dir/non-file entries here avoids walking into (and
        // double-counting) those aliases.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            subdirs.push(path);
        } else if file_type.is_file() && path.file_name().is_some_and(|n| n == "in.yaml") {
            has_in_yaml = true;
        }
    }
    if has_in_yaml {
        out.push(dir.join("in.yaml"));
    }
    for subdir in subdirs {
        collect_cases(&subdir, out);
    }
}

// --- Case checking -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FailureCategory {
    /// A case expected to parse successfully failed to parse at all (grammar gap).
    ParseErrorOnValidCase,
    /// Parsed, but the produced tree has the wrong shape (kind or seq/map length mismatch) --
    /// usually an unimplemented-grammar stub silently producing the wrong shape.
    StructuralMismatch,
    /// Parsed with the right shape, but scalar content or a resolved tag differs -- a real
    /// semantic bug rather than a missing feature.
    ContentMismatch,
    /// A case expected to fail parsing (has an `error` marker file) parsed successfully instead.
    UnexpectedSuccessOnErrorCase,
    /// The fixture itself (`in.yaml`/`test.event`) couldn't be read or didn't match the event
    /// format this harness understands -- a harness/fixture problem, not a `ya` conformance
    /// signal.
    MalformedFixture,
    /// `ya`'s parser itself panicked (e.g. an internal `winnow` invariant like "repeat parsers
    /// must always consume") rather than returning a parse error -- a real crash bug in `ya`,
    /// distinct from a clean rejection.
    ParserPanic,
}

impl FailureCategory {
    const ALL: [FailureCategory; 6] = [
        FailureCategory::ParseErrorOnValidCase,
        FailureCategory::StructuralMismatch,
        FailureCategory::ContentMismatch,
        FailureCategory::UnexpectedSuccessOnErrorCase,
        FailureCategory::MalformedFixture,
        FailureCategory::ParserPanic,
    ];
}

#[derive(Debug)]
struct CaseFailure {
    category: FailureCategory,
    message: String,
}

impl std::fmt::Display for CaseFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.category, self.message)
    }
}

fn check_case(case: &Path) -> Result<(), CaseFailure> {
    let case_dir = case.parent().unwrap();
    let error_file = case_dir.join("error");
    if error_file.exists() {
        check_error(case)
    } else {
        check_input(case, &case_dir.join("test.event"))
    }
}

fn check_error(case: &Path) -> Result<(), CaseFailure> {
    let text = read_fixture(case)?;
    match yaml_stream::<_, ContextError>.parse(ya::parse::input::Input::new(&text)) {
        Err(_) => Ok(()),
        Ok(_) => Err(CaseFailure {
            category: FailureCategory::UnexpectedSuccessOnErrorCase,
            message: "expected a parse error, but input parsed successfully".to_string(),
        }),
    }
}

fn check_input(case: &Path, event: &Path) -> Result<(), CaseFailure> {
    let text = read_fixture(case)?;
    let stream = match yaml_stream::<_, ContextError>.parse(ya::parse::input::Input::new(&text)) {
        Ok(stream) => stream,
        Err(err) => {
            return Err(CaseFailure {
                category: FailureCategory::ParseErrorOnValidCase,
                message: format!(
                    "parse error at byte offset {}: {}",
                    err.offset(),
                    err.inner()
                ),
            })
        }
    };

    let event_text = read_fixture(event)?;
    let expected = parse_oracle_stream(&event_text).map_err(|message| CaseFailure {
        category: FailureCategory::MalformedFixture,
        message,
    })?;
    let actual: Vec<ExpectedNode> = stream
        .documents()
        .iter()
        .map(|doc| node_to_expected(doc.as_node()))
        .collect();

    if expected.len() != actual.len() {
        return Err(CaseFailure {
            category: FailureCategory::StructuralMismatch,
            message: format!(
                "document count mismatch: expected {}, got {}",
                expected.len(),
                actual.len()
            ),
        });
    }
    for (i, (e, a)) in expected.iter().zip(&actual).enumerate() {
        if let Some((kind, message)) = diff_nodes(&format!("doc[{i}]"), e, a) {
            let category = match kind {
                MismatchKind::Structural => FailureCategory::StructuralMismatch,
                MismatchKind::Content => FailureCategory::ContentMismatch,
            };
            return Err(CaseFailure { category, message });
        }
    }
    Ok(())
}

thread_local! {
    static LAST_PANIC_MESSAGE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn read_fixture(path: &Path) -> Result<String, CaseFailure> {
    std::fs::read_to_string(path).map_err(|err| CaseFailure {
        category: FailureCategory::MalformedFixture,
        message: format!("failed to read {}: {err}", path.display()),
    })
}

// --- Shared tree type + structural diff --------------------------------------------------

/// A parsed YAML node, stripped of everything `value::Node` can't represent yet (presentation
/// style, anchor names) so instances from `test.event` and from `ya`'s own output can be compared
/// directly. See the module docs for why those dimensions are excluded.
#[derive(Debug, Clone, PartialEq)]
enum ExpectedNode {
    Scalar {
        tag: Option<String>,
        value: String,
    },
    Seq {
        tag: Option<String>,
        items: Vec<ExpectedNode>,
    },
    Map {
        tag: Option<String>,
        entries: Vec<(ExpectedNode, ExpectedNode)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MismatchKind {
    /// Node kind or seq/map length differ.
    Structural,
    /// Same shape, but scalar content or a resolved tag differ.
    Content,
}

/// Returns the first point of divergence between `expected` and `actual`, if any, with a
/// breadcrumb `path` for a readable failure message.
fn diff_nodes(
    path: &str,
    expected: &ExpectedNode,
    actual: &ExpectedNode,
) -> Option<(MismatchKind, String)> {
    match (expected, actual) {
        (
            ExpectedNode::Scalar { tag: et, value: ev },
            ExpectedNode::Scalar { tag: at, value: av },
        ) => {
            if ev != av {
                Some((
                    MismatchKind::Content,
                    format!("{path}: scalar value mismatch: expected {ev:?}, got {av:?}"),
                ))
            } else if et != at {
                Some((
                    MismatchKind::Content,
                    format!("{path}: scalar tag mismatch: expected {et:?}, got {at:?}"),
                ))
            } else {
                None
            }
        }
        (ExpectedNode::Seq { tag: et, items: ei }, ExpectedNode::Seq { tag: at, items: ai }) => {
            if et != at {
                return Some((
                    MismatchKind::Content,
                    format!("{path}: sequence tag mismatch: expected {et:?}, got {at:?}"),
                ));
            }
            if ei.len() != ai.len() {
                return Some((
                    MismatchKind::Structural,
                    format!(
                        "{path}: sequence length mismatch: expected {}, got {}",
                        ei.len(),
                        ai.len()
                    ),
                ));
            }
            ei.iter()
                .zip(ai)
                .enumerate()
                .find_map(|(i, (e, a))| diff_nodes(&format!("{path}[{i}]"), e, a))
        }
        (
            ExpectedNode::Map {
                tag: et,
                entries: ee,
            },
            ExpectedNode::Map {
                tag: at,
                entries: ae,
            },
        ) => {
            if et != at {
                return Some((
                    MismatchKind::Content,
                    format!("{path}: mapping tag mismatch: expected {et:?}, got {at:?}"),
                ));
            }
            if ee.len() != ae.len() {
                return Some((
                    MismatchKind::Structural,
                    format!(
                        "{path}: mapping length mismatch: expected {}, got {}",
                        ee.len(),
                        ae.len()
                    ),
                ));
            }
            for (i, ((ek, ev), (ak, av))) in ee.iter().zip(ae).enumerate() {
                if let Some(d) = diff_nodes(&format!("{path}.key[{i}]"), ek, ak) {
                    return Some(d);
                }
                if let Some(d) = diff_nodes(&format!("{path}.val[{i}]"), ev, av) {
                    return Some(d);
                }
            }
            None
        }
        (e, a) => Some((
            MismatchKind::Structural,
            format!("{path}: node kind mismatch: expected {e:?}, got {a:?}"),
        )),
    }
}

// --- ya::value::Node -> ExpectedNode -----------------------------------------------------

fn node_to_expected(node: &value::Node<'_>) -> ExpectedNode {
    let tag = tag_uri(&node.tag);
    match &node.value {
        value::Content::Empty => ExpectedNode::Scalar {
            tag,
            value: String::new(),
        },
        value::Content::Scalar(s) => ExpectedNode::Scalar {
            tag,
            value: scalar_value(s),
        },
        value::Content::Seq(items) => ExpectedNode::Seq {
            tag,
            items: items.iter().map(node_to_expected).collect(),
        },
        value::Content::Map(mapping) => ExpectedNode::Map {
            tag,
            entries: mapping
                .entries()
                .iter()
                .map(|entry| (node_to_expected(&entry.key), node_to_expected(&entry.value)))
                .collect(),
        },
    }
}

fn tag_uri(tag: &value::Tag<'_>) -> Option<String> {
    match tag {
        value::Tag::Unspecified => None,
        value::Tag::Global(s) => Some(s.to_string()),
        value::Tag::Standard(t) => Some(standard_tag_uri(*t).to_string()),
    }
}

fn standard_tag_uri(tag: value::StandardTag) -> &'static str {
    match tag {
        value::StandardTag::Map => "tag:yaml.org,2002:map",
        value::StandardTag::Seq => "tag:yaml.org,2002:seq",
        value::StandardTag::Str => "tag:yaml.org,2002:str",
        value::StandardTag::Null => "tag:yaml.org,2002:null",
        value::StandardTag::Bool => "tag:yaml.org,2002:bool",
        value::StandardTag::Int => "tag:yaml.org,2002:int",
        value::StandardTag::Float => "tag:yaml.org,2002:float",
    }
}

/// Extracts scalar content only (style is deliberately not compared, see module docs).
///
/// NOTE: once core-schema resolution (AGENT.md Phase 6) starts actually producing
/// `Null`/`Bool`/`Int`/`Float`, exact event-content comparison for those will need the original
/// source text preserved somewhere -- `test.event` records presentation text verbatim (e.g. `~`
/// stays `~`, not a canonicalized `null`), which a resolved-and-retyped scalar can't reproduce.
/// These branches are forward-compatible placeholders; `ya`'s parser doesn't produce these
/// variants yet, so they're currently unreachable.
fn scalar_value(scalar: &value::Scalar<'_>) -> String {
    match scalar {
        value::Scalar::SingleStr(s) | value::Scalar::DoubleStr(s) => s.to_string(),
        value::Scalar::Null => String::new(),
        value::Scalar::Bool(b) => b.to_string(),
        value::Scalar::Int(i) => i.to_string(),
        value::Scalar::Float(f) => f.to_string(),
    }
}

// --- test.event oracle parser -------------------------------------------------------------

type LineIter<'a> = std::iter::Peekable<std::str::Lines<'a>>;

/// Parses a full `test.event` fixture into one [`ExpectedNode`] per document in the stream.
fn parse_oracle_stream(text: &str) -> Result<Vec<ExpectedNode>, String> {
    let mut lines: LineIter = text.lines().peekable();
    expect_prefix(&mut lines, "+STR")?;
    let mut docs = Vec::new();
    loop {
        match lines.peek().copied() {
            Some(line) if line.starts_with("+DOC") => {
                lines.next();
                let mut anchors: HashMap<String, ExpectedNode> = HashMap::new();
                let node = match lines.peek().copied() {
                    // Not observed anywhere in the corpus (even an empty input yields zero
                    // documents, see AVM7), but handled defensively: a document with literally no
                    // node resolves the same way an empty plain scalar would.
                    Some(next) if next.starts_with("-DOC") => ExpectedNode::Scalar {
                        tag: None,
                        value: String::new(),
                    },
                    _ => parse_oracle_node(&mut lines, &mut anchors)?,
                };
                docs.push(node);
                expect_prefix(&mut lines, "-DOC")?;
            }
            Some("-STR") => {
                lines.next();
                break;
            }
            other => return Err(format!("unexpected line while scanning stream: {other:?}")),
        }
    }
    if let Some(trailing) = lines.next() {
        return Err(format!("trailing content after -STR: {trailing:?}"));
    }
    Ok(docs)
}

fn expect_prefix(lines: &mut LineIter, prefix: &str) -> Result<(), String> {
    match lines.next() {
        Some(line) if line.starts_with(prefix) => Ok(()),
        other => Err(format!(
            "expected a line starting with {prefix:?}, got {other:?}"
        )),
    }
}

/// Parses one node (scalar/seq/map/alias) starting at the cursor, consuming its full extent
/// (including the matching `-SEQ`/`-MAP` line), and registers its anchor (if any) for later
/// `=ALI` lookups.
fn parse_oracle_node(
    lines: &mut LineIter,
    anchors: &mut HashMap<String, ExpectedNode>,
) -> Result<ExpectedNode, String> {
    let line = *lines
        .peek()
        .ok_or("unexpected end of event stream while expecting a node")?;
    let (code, rest) = split_code(line);
    match code {
        "+MAP" => {
            lines.next();
            let (anchor, tag, _) = take_prefix_tokens(rest, true);
            let mut entries = Vec::new();
            loop {
                match lines.peek().copied() {
                    Some(l) if l.starts_with("-MAP") => {
                        lines.next();
                        break;
                    }
                    Some(_) => {
                        let key = parse_oracle_node(lines, anchors)?;
                        let value = parse_oracle_node(lines, anchors)?;
                        entries.push((key, value));
                    }
                    None => {
                        return Err("unexpected end of event stream inside a mapping".to_string())
                    }
                }
            }
            let node = ExpectedNode::Map { tag, entries };
            register_anchor(anchors, anchor, &node);
            Ok(node)
        }
        "+SEQ" => {
            lines.next();
            let (anchor, tag, _) = take_prefix_tokens(rest, true);
            let mut items = Vec::new();
            loop {
                match lines.peek().copied() {
                    Some(l) if l.starts_with("-SEQ") => {
                        lines.next();
                        break;
                    }
                    Some(_) => items.push(parse_oracle_node(lines, anchors)?),
                    None => {
                        return Err("unexpected end of event stream inside a sequence".to_string())
                    }
                }
            }
            let node = ExpectedNode::Seq { tag, items };
            register_anchor(anchors, anchor, &node);
            Ok(node)
        }
        "=VAL" => {
            lines.next();
            let (anchor, tag, rest) = take_prefix_tokens(rest, false);
            let mut chars = rest.chars();
            let style = chars
                .next()
                .ok_or_else(|| format!("=VAL line missing a style indicator: {line:?}"))?;
            if !matches!(style, ':' | '\'' | '"' | '|' | '>') {
                return Err(format!("unknown scalar style {style:?} in line {line:?}"));
            }
            let value = unescape_event_value(chars.as_str());
            let node = ExpectedNode::Scalar { tag, value };
            register_anchor(anchors, anchor, &node);
            Ok(node)
        }
        "=ALI" => {
            lines.next();
            let name = rest
                .trim_start()
                .strip_prefix('*')
                .ok_or_else(|| format!("malformed alias line: {line:?}"))?;
            anchors
                .get(name)
                .cloned()
                .ok_or_else(|| format!("alias to unknown or not-yet-defined anchor {name:?}"))
        }
        other => Err(format!(
            "expected a node-start event (+MAP/+SEQ/=VAL/=ALI), got {other:?} (line: {line:?})"
        )),
    }
}

fn register_anchor(
    anchors: &mut HashMap<String, ExpectedNode>,
    anchor: Option<String>,
    node: &ExpectedNode,
) {
    if let Some(name) = anchor {
        anchors.insert(name, node.clone());
    }
}

/// Splits a fixed 4-char event code (`+MAP`, `=VAL`, ...) from the (whitespace-trimmed) rest of
/// the line.
fn split_code(line: &str) -> (&str, &str) {
    if line.len() >= 4 {
        let (code, rest) = line.split_at(4);
        (code, rest.trim_start())
    } else {
        (line, "")
    }
}

/// Consumes a run of `&anchor`/`<tag>`/(optionally) `{}`/`[]` tokens in any order and any subset
/// -- the corpus does *not* guarantee a fixed order (e.g. case `6BFJ` has `+SEQ [] &key`, flow
/// marker before anchor) -- stopping at the first token matching none of these.
fn take_prefix_tokens(
    mut rest: &str,
    allow_flow_marker: bool,
) -> (Option<String>, Option<String>, &str) {
    let mut anchor = None;
    let mut tag = None;
    loop {
        let trimmed = rest.trim_start();
        if let Some(after) = trimmed.strip_prefix('&') {
            let (name, after) = split_token(after);
            anchor = Some(name.to_string());
            rest = after;
        } else if let Some(after) = trimmed.strip_prefix('<') {
            let end = after
                .find('>')
                .unwrap_or_else(|| panic!("malformed tag token (missing '>'): {trimmed:?}"));
            tag = Some(after[..end].to_string());
            rest = &after[end + 1..];
        } else if allow_flow_marker && (trimmed.starts_with("{}") || trimmed.starts_with("[]")) {
            rest = &trimmed[2..];
        } else {
            rest = trimmed;
            break;
        }
    }
    (anchor, tag, rest)
}

/// Splits `s` at the first whitespace character, e.g. `"foo bar"` -> `("foo", " bar")`.
fn split_token(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

/// Reverses `test.event`'s content escaping. Confirmed against the corpus: `\\` -> `\`,
/// `\n` -> newline, `\t` -> tab; literal `"`/`'` are never escaped. Hex escapes (`\x`/`\u`/`\U`)
/// are handled defensively by analogy with `src/parse/double.rs`'s escape table, though none were
/// found in this corpus checkout.
fn unescape_event_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('0') => out.push('\0'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('e') => out.push('\u{1b}'),
            Some('x') => push_hex_escape(&mut out, &mut chars, 2),
            Some('u') => push_hex_escape(&mut out, &mut chars, 4),
            Some('U') => push_hex_escape(&mut out, &mut chars, 8),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn push_hex_escape(out: &mut String, chars: &mut std::str::Chars<'_>, width: usize) {
    let hex: String = chars.by_ref().take(width).collect();
    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
        out.push(ch);
    } else {
        // Malformed/unrecognized escape: keep it literally rather than silently dropping data.
        out.push('\\');
        out.push_str(&hex);
    }
}
