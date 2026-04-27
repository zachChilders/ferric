# Milestone LSP — Language Server + VS Code Extension

> **Prerequisite:** M2.5 (all four tasks) must be complete and all tests passing before
> starting this milestone. Specifically, Task 4 (AST Public Surface) must be done —
> `ferric_common` types must already derive `Serialize + Deserialize + Clone + PartialEq`
> and the `--dump-ast` flag must exist. This milestone builds entirely on top of those
> foundations.

> **No stage internals are touched.** This milestone adds one new crate (`ferric_lsp`),
> one new tooling script (`tools/package-extension.sh`), and one `build.rs` inside
> `ferric_lsp`. Every pipeline stage is called exclusively through its public entry
> function. The blast radius of this milestone on the interpreter codebase is zero.

---

## Architecture note: two separate build concerns

This milestone has two distinct build outputs that use different mechanisms by design:

**TextMate grammar generation — lives in `build.rs`**
Pure Rust. Runs inside the normal Cargo build graph. Reads token declarations from
`ferric_common` and emits `ferric_lsp/vscode-extension/syntaxes/ferric.tmLanguage.json`.
No external tools required. Grammar stays in sync with token definitions automatically
on every `cargo build`.

**VS Code extension packaging — lives in `tools/package-extension.sh`**
Requires Node.js and `vsce`. Must be run explicitly. `cargo build` never invokes it.
This is intentional: packaging a distributable `.vsix` is a release action, not a
build action. Running `vsce package` on every `cargo build` would break every CI
environment that doesn't have Node.js installed.

To build and package everything in one command: `make extension` (see Makefile
section). `cargo build` alone is sufficient for development.

---

## Architectural constraints (additions to the existing rules)

These apply specifically to `ferric_lsp` and extend, not relax, the interpreter's
existing rules.

### LSP Rule 1 — Call only public stage entry points

`ferric_lsp` may import `ferric_common`, `ferric_lexer`, `ferric_parser`,
`ferric_resolve`, and `ferric_typecheck`. It may call only their single public entry
functions. It may not import any internal type from any stage crate.

```rust
// LEGAL
use ferric_lexer::lex;
use ferric_parser::parse;
use ferric_resolve::resolve;
use ferric_typecheck::typecheck;
use ferric_common::{LexResult, ParseResult, ResolveResult, TypeResult};

// ILLEGAL — importing stage internals
use ferric_parser::ExprParser;     // internal
use ferric_resolve::ScopeStack;    // internal
```

When a stage is replaced (e.g. `ferric_typecheck` → `ferric_infer` in M3), the LSP
import list changes by exactly one line. This is the only permitted blast radius.

### LSP Rule 2 — Stages must not panic. The LSP enforces this contract explicitly.

The interpreter architecture already requires stages to accumulate errors rather than
panic. The LSP makes this a hard runtime contract: every stage call is wrapped in
`std::panic::catch_unwind`. A stage panic is caught, converted to a `LspError::StagePanic`,
and reported as a single diagnostic at line 1. This is a safety net, not a design
goal — if a stage panics, that is a bug in the stage that must be fixed.

### LSP Rule 3 — Pipeline state is immutable and versioned

The LSP maintains a `PipelineCache` per open document. Each cache entry is an
immutable snapshot keyed by document version number. Stages are never re-run
on the same version. When a document changes, a fresh pipeline run is started for
the new version and the old entry is atomically replaced on completion. Partial
results (e.g. lex succeeded but parse failed) are stored and used — the last-good
result for each stage is always available for completion and hover, even when a
later stage has errors.

### LSP Rule 4 — Linting and formatting are injectable, not baked in

The LSP exposes two extension points that later milestones will fill:

```rust
pub trait Linter: Send + Sync {
    fn lint(&self, ast: &ParseResult, resolve: &ResolveResult) -> Vec<LintDiagnostic>;
}

pub trait Formatter: Send + Sync {
    fn format(&self, source: &str, ast: &ParseResult) -> Option<String>;
}
```

Both traits live in `ferric_lsp`. In this milestone, `NoopLinter` and `NoopFormatter`
are the only implementations. When the lint and format milestones arrive, they add a
new crate (`ferric_lint`, `ferric_fmt`) that each implement the relevant trait, and
the LSP server is constructed with the new implementation. No LSP code changes.

```rust
// This milestone: server constructed with noops
let server = LspServer::new(NoopLinter, NoopFormatter);

// Future lint milestone: one-line swap
let server = LspServer::new(FerricLinter::new(), NoopFormatter);
```

`LspServer::new` is the single configuration point. It is the LSP's equivalent of
`main.rs` in the interpreter.

---

## Project structure additions

```
ferric/
├── crates/
│   └── ferric_lsp/                    (new crate — the language server)
│       ├── Cargo.toml
│       ├── build.rs                   (generates ferric.tmLanguage.json)
│       ├── src/
│       │   ├── main.rs                (LSP binary entry point)
│       │   ├── server.rs              (LspServer — wires everything together)
│       │   ├── pipeline.rs            (PipelineCache + incremental runner)
│       │   ├── capabilities.rs        (LSP capability declarations)
│       │   ├── handlers/
│       │   │   ├── diagnostics.rs     (textDocument/publishDiagnostics)
│       │   │   ├── completion.rs      (textDocument/completion)
│       │   │   ├── hover.rs           (textDocument/hover)
│       │   │   ├── goto_def.rs        (textDocument/definition)
│       │   │   ├── document_symbols.rs (textDocument/documentSymbol)
│       │   │   └── inlay_hints.rs     (textDocument/inlayHint)
│       │   └── extension/
│       │       ├── linter.rs          (Linter trait + NoopLinter)
│       │       └── formatter.rs       (Formatter trait + NoopFormatter)
│       └── vscode-extension/          (the VS Code extension package)
│           ├── package.json
│           ├── client/
│           │   └── extension.js       (VS Code extension entry — starts LSP binary)
│           ├── syntaxes/
│           │   └── ferric.tmLanguage.json   (generated by build.rs — do not edit)
│           └── language-configuration.json
├── tools/
│   └── package-extension.sh           (runs cargo build + vsce package)
└── Makefile                           (top-level developer commands)
```

The `vscode-extension/syntaxes/ferric.tmLanguage.json` file is listed in
`.gitignore` (it is generated). Developers must run `cargo build` before opening
the extension folder in VS Code.

---

## New crate: ferric_lsp

### Cargo.toml

```toml
[package]
name    = "ferric-lsp"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ferric-lsp"
path = "src/main.rs"

[dependencies]
ferric_common    = { path = "../ferric_common" }
ferric_lexer     = { path = "../ferric_lexer" }
ferric_parser    = { path = "../ferric_parser" }
ferric_resolve   = { path = "../ferric_resolve" }
ferric_typecheck = { path = "../ferric_typecheck" }

tower-lsp  = "0.20"    # LSP protocol + JSON-RPC over stdio
tokio      = { version = "1", features = ["full"] }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
dashmap    = "5"        # concurrent document state — no Mutex required

[build-dependencies]
ferric_common = { path = "../ferric_common" }
serde_json    = "1"
```

`tower-lsp` provides the JSON-RPC transport and LSP message routing.
`ferric_lsp` is an async binary (`tokio`). The pipeline is run on a blocking
thread pool via `tokio::task::spawn_blocking` so that synchronous stage code
never blocks the async event loop.

### build.rs — TextMate grammar generation

`build.rs` generates `vscode-extension/syntaxes/ferric.tmLanguage.json` from the
token declarations in `ferric_common`. It does **not** call `lex()` at build time —
it reads the token type definitions and keyword lists that `ferric_common` exposes
as constants.

**Required addition to `ferric_common`** (the only change to an existing crate
in this milestone):

```rust
// ferric_common/src/keywords.rs  — new file
// A single source of truth for all Ferric keywords and operators.
// Consumed by both the lexer (at runtime) and build.rs (at build time).
// Adding a keyword here automatically updates the TextMate grammar.

pub const KEYWORDS: &[&str] = &[
    "let", "mut", "fn", "return",
    "if", "else", "while", "loop",
    "break", "continue", "true", "false",
    "require",                          // added in M2.5
];

pub const TYPE_KEYWORDS: &[&str] = &[
    "Int", "Float", "Bool", "Str", "Unit",
];

pub const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%",
    "==", "!=", "<", ">", "<=", ">=",
    "&&", "||", "!",
    "=",
];
```

This file is a new addition. The lexer is updated to import from here instead of
duplicating string literals — this is purely a refactor within `ferric_lexer`'s
internals and does not touch any public signature. The lexer's public entry point
`pub fn lex(source: &str, interner: &mut Interner) -> LexResult` is unchanged.

`build.rs` imports `ferric_common::keywords` and writes the grammar file:

```rust
// ferric_lsp/build.rs
use ferric_common::keywords::{KEYWORDS, TYPE_KEYWORDS, OPERATORS};
use std::{env, fs, path::PathBuf};

fn main() {
    // Tell Cargo to re-run build.rs if keywords change
    println!("cargo:rerun-if-changed=../ferric_common/src/keywords.rs");

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("vscode-extension/syntaxes");
    fs::create_dir_all(&out_dir).unwrap();

    let grammar = build_grammar();
    fs::write(out_dir.join("ferric.tmLanguage.json"), grammar).unwrap();
}

fn build_grammar() -> String {
    // Build the TextMate grammar JSON structure.
    // Full implementation in the stage implementations section below.
}
```

The `cargo:rerun-if-changed` directive means Cargo re-runs `build.rs` (and therefore
regenerates the grammar) whenever `keywords.rs` changes. No manual step required.

**Sync guarantee:** Any keyword added to `KEYWORDS` in `ferric_common` is
automatically present in the grammar on the next `cargo build`. The lexer and the
grammar cannot drift because they both derive from the same constant array.

---

## Pipeline runner: pipeline.rs

The pipeline runner is the heart of the LSP. It runs the interpreter pipeline on
demand and caches results by document version.

### PipelineSnapshot

```rust
// The result of one full pipeline run on one version of a document.
// Immutable once created. Stored in PipelineCache indexed by (uri, version).
pub struct PipelineSnapshot {
    pub version:  i32,
    pub source:   String,

    // Each stage result is Option — None if that stage was not reached
    // (because a preceding stage had errors that prevent continuation).
    // The last-good result from a previous snapshot is used for completions
    // and hover when the current snapshot has None for that stage.
    pub lex:      Option<LexResult>,
    pub parse:    Option<ParseResult>,
    pub resolve:  Option<ResolveResult>,
    pub typecheck: Option<TypeResult>,
}

impl PipelineSnapshot {
    // All diagnostics from all stages, flattened and converted to LSP format.
    pub fn all_diagnostics(&self) -> Vec<lsp_types::Diagnostic> { ... }
}
```

### PipelineCache

```rust
pub struct PipelineCache {
    // uri → (current_snapshot, last_good_snapshot_per_stage)
    documents: DashMap<String, DocumentState>,
}

struct DocumentState {
    current:    Arc<PipelineSnapshot>,
    // last_good[stage] is the most recent snapshot where that stage succeeded.
    // Used for completions/hover when the current snapshot has errors.
    last_good:  [Option<Arc<PipelineSnapshot>>; 4],  // lex, parse, resolve, typecheck
}
```

### Running the pipeline

```rust
// Called on every document change. Runs on a blocking thread pool.
pub fn run_pipeline(source: &str, version: i32) -> PipelineSnapshot {
    let mut snapshot = PipelineSnapshot::new(version, source.to_string());
    let mut interner = Interner::default();

    // Stage 1: Lex
    let lex_result = catch_stage(|| ferric_lexer::lex(source, &mut interner));
    let lex_ok = lex_result.as_ref().map(|r| r.errors.is_empty()).unwrap_or(false);
    snapshot.lex = lex_result;
    if !lex_ok { return snapshot; }  // parse cannot run without a clean lex

    // Stage 2: Parse
    // Note: parse runs even if lex has errors — the lexer emits error tokens
    // that the parser can recover from. The guard above only blocks parse if
    // the lex result itself is missing (i.e. a stage panic occurred).
    let parse_result = catch_stage(|| ferric_parser::parse(snapshot.lex.as_ref().unwrap()));
    snapshot.parse = parse_result;
    if snapshot.parse.is_none() { return snapshot; }

    // Stage 3: Resolve
    let resolve_result = catch_stage(|| {
        ferric_resolve::resolve(snapshot.parse.as_ref().unwrap())
    });
    snapshot.resolve = resolve_result;
    if snapshot.resolve.is_none() { return snapshot; }

    // Stage 4: Typecheck
    let type_result = catch_stage(|| ferric_typecheck::typecheck(
        snapshot.parse.as_ref().unwrap(),
        snapshot.resolve.as_ref().unwrap(),
    ));
    snapshot.typecheck = type_result;

    snapshot
}

fn catch_stage<T, F: FnOnce() -> T + std::panic::UnwindSafe>(f: F) -> Option<T> {
    std::panic::catch_unwind(f).ok()
}
```

**Note on lex/parse error gating:** Unlike the CLI (which exits on any lex error),
the LSP is more permissive. The lexer is designed to accumulate errors and continue,
producing a partial token stream. The parser is similarly designed. This means the
LSP can provide completions and hover even in files with syntax errors — it uses the
partial parse result. The gating above only applies to outright stage panics (which
are bugs). Errors in `LexResult::errors` and `ParseResult::errors` do not block
later stages.

---

## LSP capabilities

This milestone implements a focused set of capabilities. More are added as
later milestones introduce the resolved information they depend on.

```rust
// capabilities.rs
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        // Diagnostics are push-based (publishDiagnostics), not capability-declared
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), ":".into()]),
            ..Default::default()
        }),
        hover_provider:           Some(HoverProviderCapability::Simple(true)),
        definition_provider:      Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        inlay_hint_provider:      Some(OneOf::Left(true)),
        ..Default::default()
    }
}
```

### Diagnostics (textDocument/publishDiagnostics)

All errors from all stages are converted to LSP `Diagnostic` objects and pushed
to the client after every pipeline run. The conversion is one-to-one: each
`LexError`, `ParseError`, `ResolveError`, and `TypeError` (all of which carry
`Span` per Rule 5) becomes one LSP diagnostic.

```rust
fn ferric_span_to_lsp_range(span: Span, source: &str) -> lsp_types::Range {
    // Convert byte-offset Span to line/column Range.
    // Source text is walked once to build a line-start-offset table.
}

fn severity_for_stage(stage: Stage) -> DiagnosticSeverity {
    DiagnosticSeverity::ERROR  // all stage errors are errors; warnings come from the linter
}
```

Warnings from `require(warn)` mode (M2.5 Task 2) use `DiagnosticSeverity::WARNING`.

### Completion (textDocument/completion)

Completions are drawn from the resolve result. If the current snapshot has no
resolve result (the file has errors), the last-good resolve result is used.

Sources of completion items:
- All `DefId` entries in `ResolveResult::resolutions` (local variables, function names)
- All keywords from `ferric_common::keywords::KEYWORDS`
- All stdlib function names from a static list in `ferric_lsp` (stdlib names do not
  appear in `ResolveResult` since they are injected by the VM, not resolved by the
  resolver — a static list is the correct approach here)

Completion does not require the type result in this milestone. Type-aware completions
(method completions on `.`, field completions on structs) are added when M4 (ADTs)
is complete, at which point `TypeResult` provides the necessary type information.

### Hover (textDocument/hover)

When the cursor is on an identifier, hover shows the type of that identifier.
The type is drawn from `TypeResult::node_types` keyed by `NodeId`.

The identifier under the cursor is resolved by walking `ParseResult::items` to
find the `Expr::Ident` whose `Span` contains the cursor position, then looking
up its `NodeId` in `ResolveResult::resolutions` to get the `DefId`, then looking
up the `DefId` in `TypeResult::node_types`.

If the type result is not available, hover shows the resolved name only.

```
// Hover output format (rendered as Markdown in VS Code):
**x**: Int
```

### Go-to Definition (textDocument/definition)

When the cursor is on an identifier, returns the location of its definition.
Uses `ResolveResult::resolutions` to map the cursor's `NodeId` → `DefId`, then
walks the AST to find the definition site of that `DefId`.

Returns a single `Location` (one definition per name, since Ferric has no overloading
until traits arrive in M5). Returns `null` if the name is not in scope (parse error
case) or if it is a stdlib name (no source location).

### Document Symbols (textDocument/documentSymbol)

Returns all top-level function and `let` binding names from `ParseResult::items`.
These appear in VS Code's outline panel and breadcrumb navigation.

Each symbol includes its `Span` (already on every AST item per Rule 5), converted
to an LSP `Range`. No resolve or type information is needed.

### Inlay Hints (textDocument/inlayHint)

Inlay hints render inferred type information inline in the editor as grayed-out
decorations, without modifying the source file. For example, given:

```rust
let x = fibonacci(n: 10)
```

The editor displays:

```
let x: Int = fibonacci(n: 10)
      ^^^^
      hint — not real text
```

This is the same feature rust-analyzer provides for Rust. It requires no new
pipeline work — `TypeResult::node_types` already contains the inferred type for
every AST node.

#### What gets a hint

Hints are shown only where type information is non-obvious and not already
written by the user. The rule is: **a hint appears only when the binding or
parameter has no explicit type annotation in the source.**

Two sites emit hints:

**`let` bindings without annotations:**
```rust
let x = 5          // hint: `: Int`
let y: Int = 5     // no hint — annotation already present
let mut z = 3.14   // hint: `: Float`
```

**Function parameters in closures** (not top-level `fn` — those must be annotated):
```rust
let doubled = nums.map(|x| x * 2)
//                      ^ hint: `: Int`
```

Hints are suppressed entirely if the type result is unavailable (file has type
errors) — a wrong hint is worse than no hint. The last-good type result is **not**
used here, unlike completions and hover. Stale type hints on actively-edited code
are confusing.

#### Ty pretty-printer — addition to ferric_common

Inlay hints require formatting a `Ty` as a human-readable string. This is also
used by the hover handler. A `Display` implementation is added to `ferric_common`:

```rust
// ferric_common/src/types.rs — add alongside the Ty definition
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int           => write!(f, "Int"),
            Ty::Float         => write!(f, "Float"),
            Ty::Bool          => write!(f, "Bool"),
            Ty::Str           => write!(f, "Str"),
            Ty::Unit          => write!(f, "Unit"),
            Ty::Fn { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Ty::Unknown => write!(f, "_"),   // escape hatch — removed in M3
        }
    }
}
```

This `Display` impl is in `ferric_common` because `Ty` lives there. It is the
only formatting concern that belongs in `ferric_common` — all other rendering
(error messages, diagnostics) lives in `ferric_diagnostics`. As `Ty` gains new
variants in later milestones (M4 adds `Tuple`, `Struct`, `Enum`; M5 adds trait
bounds), each new variant must add a corresponding `Display` arm. This is enforced
by the exhaustiveness checker — a missing arm is a compile error.

#### Handler implementation

```rust
// ferric_lsp/src/handlers/inlay_hints.rs

pub fn inlay_hints(
    snapshot: &PipelineSnapshot,
    range:    lsp_types::Range,
) -> Vec<lsp_types::InlayHint> {
    // Hints require a current (not last-good) type result.
    // If unavailable, return empty — stale hints are worse than none.
    let (parse, types) = match (&snapshot.parse, &snapshot.typecheck) {
        (Some(p), Some(t)) => (p, t),
        _ => return vec![],
    };

    let mut hints = Vec::new();

    for item in &parse.items {
        collect_hints_for_item(item, types, &snapshot.source, range, &mut hints);
    }

    hints
}

fn collect_hints_for_item(
    item:   &Item,
    types:  &TypeResult,
    source: &str,
    range:  lsp_types::Range,
    hints:  &mut Vec<lsp_types::InlayHint>,
) {
    match item {
        Item::Let(let_binding) if let_binding.annotation.is_none() => {
            // Unannotated let binding — look up the type of the bound expression.
            if let Some(ty) = types.node_types.get(&let_binding.node_id) {
                let position = span_end_to_lsp_position(let_binding.name_span, source);
                if position_in_range(position, range) {
                    hints.push(lsp_types::InlayHint {
                        position,
                        label: lsp_types::InlayHintLabel::String(format!(": {ty}")),
                        kind:  Some(lsp_types::InlayHintKind::TYPE),
                        ..Default::default()
                    });
                }
            }
        }
        Item::Fn(fn_def) => {
            // Recurse into function body for nested let bindings and closures.
            collect_hints_for_block(&fn_def.body, types, source, range, hints);
        }
        _ => {}
    }
}
```

The hint position is placed immediately after the binding name — `let x▌ = 5`
where `▌` marks the insertion point. The LSP position is the end of the name
span, not the start of the `=` token. VS Code renders it as `let x: Int = 5`
with `: Int` grayed out.

#### Interaction with hover

Hover and inlay hints show the same type information from the same source
(`TypeResult::node_types`). They use the same `Ty` `Display` impl. The difference
is that hover is on-demand (cursor position) while inlay hints cover the full
visible range. Both call into the same span-to-position utility in `pipeline.rs`.

---

## Extension points for lint and format milestones

### Linter trait

```rust
// ferric_lsp/src/extension/linter.rs

pub struct LintDiagnostic {
    pub span:     Span,
    pub message:  String,
    pub severity: LintSeverity,
    pub code:     Option<String>,   // e.g. "F0042" for rule-based diagnostics
}

pub enum LintSeverity { Warning, Error, Info, Hint }

pub trait Linter: Send + Sync {
    /// Called after a successful pipeline run with all available stage outputs.
    /// Returns an empty Vec if nothing to report.
    fn lint(
        &self,
        ast:     &ParseResult,
        resolve: &ResolveResult,
        types:   &TypeResult,
    ) -> Vec<LintDiagnostic>;
}

pub struct NoopLinter;
impl Linter for NoopLinter {
    fn lint(&self, _: &ParseResult, _: &ResolveResult, _: &TypeResult) -> Vec<LintDiagnostic> {
        vec![]
    }
}
```

`Linter::lint` is only called when all four pipeline stages succeed. Partial-result
linting (e.g. lint after parse but before resolve) is not supported in this design —
a linter that needs only the AST should still implement `Linter` and ignore the
resolve and type arguments.

When the lint milestone arrives, a new crate `ferric_lint` provides a `FerricLinter`
struct that implements `Linter`. The LSP server is reconstructed with it. No other
LSP code changes.

### Formatter trait

```rust
// ferric_lsp/src/extension/formatter.rs

pub trait Formatter: Send + Sync {
    /// Returns the fully formatted source text, or None if formatting was skipped
    /// (e.g. because the file has syntax errors that prevent safe formatting).
    fn format(
        &self,
        source: &str,
        ast:    &ParseResult,
    ) -> Option<String>;
}

pub struct NoopFormatter;
impl Formatter for NoopFormatter {
    fn format(&self, _: &str, _: &ParseResult) -> Option<String> { None }
}
```

The LSP handles `textDocument/formatting` by calling `formatter.format(...)` and
converting the result to an LSP `TextEdit` covering the entire document. This handler
is registered in `capabilities.rs` only when the injected `Formatter` is not a
`NoopFormatter` — this prevents VS Code from showing "Format Document" in the menu
until the feature is actually implemented.

```rust
// capabilities.rs — formatting capability is conditional
document_formatting_provider: Some(OneOf::Left(!formatter.is_noop())),
```

`Formatter` gets a `fn is_noop(&self) -> bool` default method that returns `false`.
`NoopFormatter` overrides it to return `true`.

---

## TextMate grammar: full specification

The generated `ferric.tmLanguage.json` covers:

- **Keywords** — from `KEYWORDS` constant: control flow colored distinctly
- **Type names** — from `TYPE_KEYWORDS` constant
- **Operators** — from `OPERATORS` constant
- **String literals** — `"..."` with escape sequences
- **Integer literals** — decimal digits
- **Float literals** — decimal with `.`
- **Boolean literals** — `true`, `false`
- **Single-line comments** — `// ...`
- **Function definitions** — `fn <name>(` — function name captured as a distinct scope
- **Shell expressions** — `$` at start of expression position (best-effort, not context-sensitive)
- **Shell interpolation** — `@{` ... `}` inside shell lines

The grammar uses standard TextMate scopes so any VS Code theme provides reasonable
colors with zero theme-specific configuration:

| Token kind      | TextMate scope                        |
|-----------------|---------------------------------------|
| Keywords        | `keyword.control.ferric`              |
| Type keywords   | `storage.type.ferric`                 |
| String literals | `string.quoted.double.ferric`         |
| Comments        | `comment.line.double-slash.ferric`    |
| Integers        | `constant.numeric.integer.ferric`     |
| Floats          | `constant.numeric.float.ferric`       |
| Booleans        | `constant.language.boolean.ferric`    |
| Function names  | `entity.name.function.ferric`         |
| Operators       | `keyword.operator.ferric`             |
| Shell `$`       | `keyword.other.shell.ferric`          |
| Shell `@{...}`  | `variable.other.interpolated.ferric`  |

### build.rs implementation

```rust
// ferric_lsp/build.rs

use ferric_common::keywords::{KEYWORDS, TYPE_KEYWORDS, OPERATORS};
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../ferric_common/src/keywords.rs");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_path = manifest_dir
        .join("vscode-extension/syntaxes/ferric.tmLanguage.json");
    fs::create_dir_all(out_path.parent().unwrap()).unwrap();

    let grammar = build_grammar();
    let json    = serde_json::to_string_pretty(&grammar).unwrap();
    fs::write(&out_path, json).unwrap();

    eprintln!("cargo:warning=Generated TextMate grammar at {}", out_path.display());
}

fn build_grammar() -> Value {
    // Build keyword alternation pattern from the constant arrays.
    // \b anchors prevent matching "letter" inside "letterhead".
    let kw_pattern  = format!(r"\b({})\b", KEYWORDS.join("|"));
    let ty_pattern  = format!(r"\b({})\b", TYPE_KEYWORDS.join("|"));
    // Operators need no word boundaries — they are punctuation.
    let op_escaped: Vec<String> = OPERATORS.iter()
        .map(|op| regex_escape(op))
        .collect();
    let op_pattern  = op_escaped.join("|");

    json!({
        "$schema": "https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json",
        "name": "Ferric",
        "scopeName": "source.ferric",
        "fileTypes": ["fe"],
        "patterns": [
            { "include": "#comments" },
            { "include": "#strings" },
            { "include": "#shell-expressions" },
            { "include": "#keywords" },
            { "include": "#type-keywords" },
            { "include": "#booleans" },
            { "include": "#functions" },
            { "include": "#floats" },
            { "include": "#integers" },
            { "include": "#operators" }
        ],
        "repository": {
            "comments": {
                "name": "comment.line.double-slash.ferric",
                "match": "//.*$"
            },
            "strings": {
                "name": "string.quoted.double.ferric",
                "begin": "\"",
                "end": "\"",
                "patterns": [{
                    "name": "constant.character.escape.ferric",
                    "match": r"\\."
                }]
            },
            "shell-expressions": {
                "comment": "Shell lines: $ command @{interp}",
                "begin": r"(?<![a-zA-Z0-9_])\$\s",
                "end": r"$",
                "name": "meta.shell-expression.ferric",
                "beginCaptures": {
                    "0": { "name": "keyword.other.shell.ferric" }
                },
                "patterns": [{
                    "name": "variable.other.interpolated.ferric",
                    "begin": r"@\{",
                    "end": r"\}",
                    "patterns": [{ "include": "$self" }]
                }]
            },
            "keywords": {
                "name": "keyword.control.ferric",
                "match": kw_pattern
            },
            "type-keywords": {
                "name": "storage.type.ferric",
                "match": ty_pattern
            },
            "booleans": {
                "name": "constant.language.boolean.ferric",
                "match": r"\b(true|false)\b"
            },
            "functions": {
                "comment": "Highlight the name in `fn name(`",
                "match": r"\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*(?=\()",
                "captures": {
                    "1": { "name": "entity.name.function.ferric" }
                }
            },
            "floats": {
                "name": "constant.numeric.float.ferric",
                "match": r"\b[0-9]+\.[0-9]+\b"
            },
            "integers": {
                "name": "constant.numeric.integer.ferric",
                "match": r"\b[0-9]+\b"
            },
            "operators": {
                "name": "keyword.operator.ferric",
                "match": op_pattern
            }
        }
    })
}

fn regex_escape(s: &str) -> String {
    s.chars().map(|c| match c {
        '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' |
        '\\' | '^' | '$' | '|' => format!("\\{c}"),
        c => c.to_string(),
    }).collect()
}
```

---

## VS Code extension

### package.json

```json
{
    "name": "ferric-lang",
    "displayName": "Ferric",
    "description": "Language support for the Ferric programming language",
    "version": "0.1.0",
    "engines": { "vscode": "^1.75.0" },
    "categories": ["Programming Languages"],
    "main": "./client/extension.js",
    "contributes": {
        "languages": [{
            "id": "ferric",
            "aliases": ["Ferric", "ferric"],
            "extensions": [".fe"],
            "configuration": "./language-configuration.json"
        }],
        "grammars": [{
            "language": "ferric",
            "scopeName": "source.ferric",
            "path": "./syntaxes/ferric.tmLanguage.json"
        }]
    },
    "activationEvents": ["onLanguage:ferric"],
    "dependencies": {
        "vscode-languageclient": "^9.0.0"
    }
}
```

### language-configuration.json

```json
{
    "comments": {
        "lineComment": "//"
    },
    "brackets": [
        ["{", "}"],
        ["[", "]"],
        ["(", ")"]
    ],
    "autoClosingPairs": [
        { "open": "{", "close": "}" },
        { "open": "[", "close": "]" },
        { "open": "(", "close": ")" },
        { "open": "\"", "close": "\"" }
    ],
    "surroundingPairs": [
        ["{", "}"], ["[", "]"], ["(", ")"], ["\"", "\""]
    ],
    "indentationRules": {
        "increaseIndentPattern": "\\{\\s*$",
        "decreaseIndentPattern": "^\\s*\\}"
    }
}
```

### client/extension.js

```javascript
const { workspace, window } = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');
const path = require('path');

let client;

function activate(context) {
    // The LSP binary is bundled next to the extension at package time.
    // During development, point FERRIC_LSP_PATH to a debug build.
    const lspBinary = process.env.FERRIC_LSP_PATH
        || path.join(context.extensionPath, 'bin', 'ferric-lsp');

    const serverOptions = {
        run:   { command: lspBinary, transport: TransportKind.stdio },
        debug: { command: lspBinary, transport: TransportKind.stdio,
                 args: ['--log-level', 'debug'] },
    };

    const clientOptions = {
        documentSelector: [{ scheme: 'file', language: 'ferric' }],
        synchronize: {
            fileEvents: workspace.createFileSystemWatcher('**/*.fe'),
        },
    };

    client = new LanguageClient('ferric-lsp', 'Ferric Language Server',
                                serverOptions, clientOptions);
    client.start();
}

function deactivate() {
    return client?.stop();
}

module.exports = { activate, deactivate };
```

The extension starts the `ferric-lsp` binary over stdio. The binary and extension
communicate via LSP JSON-RPC. The extension has no knowledge of the pipeline or
any Ferric internals — it is a thin transport wrapper.

---

## Packaging script

### tools/package-extension.sh

```bash
#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT_DIR="$REPO_ROOT/crates/ferric_lsp/vscode-extension"

echo "==> Building ferric-lsp (release)..."
cargo build --release --package ferric-lsp -p ferric-lsp

# build.rs has already generated the TextMate grammar at this point.

echo "==> Copying LSP binary into extension..."
mkdir -p "$EXT_DIR/bin"
cp "$REPO_ROOT/target/release/ferric-lsp" "$EXT_DIR/bin/ferric-lsp"

echo "==> Installing extension dependencies..."
cd "$EXT_DIR"
npm install

echo "==> Packaging .vsix..."
npx vsce package --out "$REPO_ROOT/ferric-lang.vsix"

echo ""
echo "Done. Install with: code --install-extension $REPO_ROOT/ferric-lang.vsix"
```

### Makefile

```makefile
.PHONY: build test lsp extension clean

build:
	cargo build

test:
	cargo test

lsp:
	cargo build --package ferric-lsp

# Requires Node.js and vsce (npm install -g @vscode/vsce)
extension:
	./tools/package-extension.sh

clean:
	cargo clean
	rm -f ferric-lang.vsix
	rm -f crates/ferric_lsp/vscode-extension/bin/ferric-lsp
```

---

## Stage implementations

### server.rs — LspServer

```rust
pub struct LspServer {
    cache:     Arc<PipelineCache>,
    linter:    Arc<dyn Linter>,
    formatter: Arc<dyn Formatter>,
    client:    Arc<Client>,
}

impl LspServer {
    pub fn new(
        client:    Client,
        linter:    impl Linter + 'static,
        formatter: impl Formatter + 'static,
    ) -> Self {
        LspServer {
            cache:     Arc::new(PipelineCache::new()),
            linter:    Arc::new(linter),
            formatter: Arc::new(formatter),
            client:    Arc::new(client),
        }
    }
}
```

`tower-lsp` requires a `#[tower_lsp::async_trait]` impl of `LanguageServer`. The
impl lives in `server.rs` and dispatches to the handler modules. Each handler
receives an `Arc<PipelineSnapshot>` (or the last-good snapshot for that stage) —
it never triggers a pipeline run. Pipeline runs are triggered exclusively by document
change notifications.

### main.rs

```rust
#[tokio::main]
async fn main() {
    let stdin  = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| {
        LspServer::new(client, NoopLinter, NoopFormatter)
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
```

When the lint milestone arrives, this becomes:

```rust
LspServer::new(client, FerricLinter::new(), NoopFormatter)
```

That is the entire change to `main.rs`.

---

## Minor change to ferric_common (the only existing crate touched)

As specified in the `build.rs` section: add `crates/ferric_common/src/keywords.rs`
containing `KEYWORDS`, `TYPE_KEYWORDS`, and `OPERATORS` as `pub const` slices.

Update `ferric_common/src/lib.rs` to `pub mod keywords;`.

Update `ferric_lexer` internals to reference `ferric_common::keywords::KEYWORDS`
instead of duplicating the keyword strings. This is an internal refactor — the
lexer's public signature `pub fn lex(source: &str, interner: &mut Interner) -> LexResult`
is unchanged. No other stage is touched.

**This is the total blast radius on existing crates: one new file in `ferric_common`,
one `pub mod` line, and a refactor inside `ferric_lexer`'s private keyword-matching
code.**

---

## Replacement log entry

| Milestone | Crate added       | Blast radius on existing crates                          |
|-----------|-------------------|----------------------------------------------------------|
| LSP       | `ferric_lsp`      | `ferric_common`: +`keywords.rs`, +`pub mod keywords`, +`Display for Ty` |
|           |                   | `ferric_lexer`: internal keyword refactor (no public API change) |

---

## Done when

**Core server:**
- [ ] `ferric-lsp` binary starts without error and accepts LSP connections over stdio
- [ ] `LspServer::new(client, linter, formatter)` is the single configuration point
- [ ] `Linter` and `Formatter` traits exist; `NoopLinter` and `NoopFormatter` are the defaults
- [ ] Pipeline runs on a `spawn_blocking` thread — async event loop never blocked
- [ ] Stage panics are caught by `catch_unwind` and reported as a single diagnostic at line 1

**Pipeline cache:**
- [ ] `PipelineCache` stores one `PipelineSnapshot` per (uri, version)
- [ ] Last-good snapshot per stage is tracked and used for completions/hover when current snapshot has errors
- [ ] A document change triggers exactly one pipeline run for the new version
- [ ] A document open triggers an immediate pipeline run

**Diagnostics:**
- [ ] All `LexError`, `ParseError`, `ResolveError`, `TypeError` items are pushed as LSP diagnostics
- [ ] Every diagnostic has a correct line/column range (derived from `Span`)
- [ ] `require(warn)` failures appear as `DiagnosticSeverity::WARNING`
- [ ] No duplicate diagnostics are pushed for the same version

**Completions:**
- [ ] Keywords from `ferric_common::keywords::KEYWORDS` appear in completion
- [ ] Local variable and function names from `ResolveResult` appear in completion
- [ ] Stdlib function names appear in completion
- [ ] Completions work even when the file has errors (last-good resolve result used)

**Hover:**
- [ ] Hovering an identifier shows its type from `TypeResult`
- [ ] Hovering an identifier with no type result shows its resolved name
- [ ] Hovering a keyword or literal returns no hover (not an error)

**Go-to definition:**
- [ ] Cursor on a local variable or function navigates to its definition site
- [ ] Cursor on a stdlib name returns null (no source location)
- [ ] Cursor on an out-of-scope name returns null (does not crash)

**Document symbols:**
- [ ] All top-level `fn` definitions appear in the outline
- [ ] All top-level `let` bindings appear in the outline
- [ ] Symbol ranges are correct

**Inlay hints:**
- [ ] Unannotated `let` bindings show an inferred type hint immediately after the binding name
- [ ] `let` bindings with explicit annotations show no hint
- [ ] Closure parameters show inferred type hints
- [ ] Hints are suppressed entirely when `TypeResult` is unavailable — no stale hints
- [ ] Hint label format is `: Ty` using the `Ty` `Display` impl from `ferric_common`
- [ ] `Ty::Display` is implemented for all current `Ty` variants and compile-errors on missing arms
- [ ] Hints are correctly filtered to the requested LSP range — off-screen hints are not returned
- [ ] Adding a new `Ty` variant in a later milestone without a `Display` arm is a compile error

**TextMate grammar:**
- [ ] `ferric.tmLanguage.json` is generated by `cargo build` — not committed to source control
- [ ] All keywords in `KEYWORDS` are highlighted as `keyword.control.ferric`
- [ ] All type keywords in `TYPE_KEYWORDS` are highlighted as `storage.type.ferric`
- [ ] String literals highlight correctly including escape sequences
- [ ] Comments highlight correctly
- [ ] Shell `$` expressions highlight the `$` and `@{...}` interpolations distinctly
- [ ] Adding a keyword to `KEYWORDS` and running `cargo build` updates the grammar automatically
- [ ] `cargo:rerun-if-changed` directive is set so Cargo re-runs `build.rs` only when `keywords.rs` changes

**VS Code extension:**
- [ ] `package.json` associates `.fe` files with the `ferric` language
- [ ] `language-configuration.json` provides bracket matching and auto-close pairs
- [ ] `client/extension.js` starts `ferric-lsp` over stdio and connects as an LSP client
- [ ] `make extension` produces a `.vsix` file
- [ ] Installing the `.vsix` provides syntax highlighting and LSP features in VS Code
- [ ] `FERRIC_LSP_PATH` env var allows pointing the extension at a dev binary

**Architecture compliance:**
- [ ] `ferric_lsp` imports only public entry functions from stage crates — no internal types
- [ ] `ferric_common::keywords` is the single source of truth for keywords — lexer and grammar both derive from it
- [ ] Replacing `ferric_typecheck` with `ferric_infer` (M3) requires changing exactly one import in `ferric_lsp`
- [ ] Adding `ferric_lint` (future) requires zero changes to `ferric_lsp` beyond `LspServer::new(client, FerricLinter::new(), ...)`
- [ ] No `Rc`, `RefCell`, or non-`Send` types are introduced
- [ ] All new error types carry `Span` (Rule 5)