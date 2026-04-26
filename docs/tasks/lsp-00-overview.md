# Milestone LSP — Task Index

This milestone adds a language server (`ferric_lsp` crate) and a VS Code extension.
The full spec lives in `lsp.md`. The work is split into seven agent-manageable
tasks below.

> **Prerequisite:** M2.5 (all four tasks) complete and all tests passing. In
> particular, `ferric_common` AST types must derive `Serialize + Deserialize +
> Clone + PartialEq` (M2.5 Task 4).

## Task DAG

```
01 (ferric_common additions)
   ├─► 02 (crate skeleton + pipeline + traits + capabilities)
   │      ├─► 04 (diagnostics + document symbols)
   │      ├─► 05 (completion + hover + goto-def)
   │      └─► 06 (inlay hints)
   ├─► 03 (build.rs — TextMate grammar)
   │
   └─► 07 (VS Code extension + packaging)
              ▲
              └── needs 02 binary to exist for end-to-end test
```

Tasks 04, 05, 06 may proceed in parallel after 02. Task 03 may run in parallel
with 02. Task 07 is independent of 03–06 except for the final end-to-end install
test.

## Task list

| # | File                                         | Scope                                             |
|---|----------------------------------------------|---------------------------------------------------|
| 1 | `lsp-01-ferric-common.md`                    | `keywords` module, `Display for Ty`, lexer refactor |
| 2 | `lsp-02-crate-skeleton.md`                   | `ferric_lsp` crate, pipeline, traits, capabilities |
| 3 | `lsp-03-textmate-grammar.md`                 | `build.rs` that generates `ferric.tmLanguage.json` |
| 4 | `lsp-04-diagnostics-symbols.md`              | `publishDiagnostics` + `documentSymbol` handlers   |
| 5 | `lsp-05-completion-hover-goto.md`            | `completion`, `hover`, `definition` handlers       |
| 6 | `lsp-06-inlay-hints.md`                      | `inlayHint` handler                                |
| 7 | `lsp-07-vscode-extension.md`                 | VS Code extension + `package-extension.sh` + Makefile |

## Architectural rules (apply to every task)

These extend the interpreter rules in `CLAUDE.md`. Every task must comply.

1. **`ferric_lsp` calls only public stage entry points.** Legal imports:
   `ferric_common`, and `lex`/`parse`/`resolve_with_natives`/`typecheck` from
   their respective crates. Importing internal types from any stage is illegal.
2. **Stages must not panic.** Every stage call is wrapped in
   `std::panic::catch_unwind` and the panic is reported as a single diagnostic
   at line 1.
3. **Pipeline state is immutable and versioned.** One `PipelineSnapshot` per
   `(uri, version)`. Stages are never re-run on the same version. Last-good
   snapshots are kept per stage for use by handlers when the current snapshot
   has errors.
4. **Linting and formatting are injectable, not baked in.** `Linter` and
   `Formatter` traits live in `ferric_lsp`. The only implementations in this
   milestone are `NoopLinter` and `NoopFormatter`. Future lint/format milestones
   add a new crate that implements the trait — no LSP code changes.
5. **No `Rc`, `RefCell`, or non-`Send` types.** The LSP is async (`tokio`) so
   shared state must be `Send + Sync`.
6. **Every error type carries a `Span`** (Rule 5 from `CLAUDE.md`).

## Stage I/O contracts the LSP relies on

These match the contracts in `CLAUDE.md`. The LSP calls them verbatim.

```rust
pub fn lex(source: &str, interner: &mut Interner) -> LexResult;
pub fn parse(lex: &LexResult) -> ParseResult;
pub fn resolve_with_natives(ast: &ParseResult, native_symbols: &[Symbol]) -> ResolveResult;
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult, interner: &Interner) -> TypeResult;
```

If a stage signature changes in the future, the LSP import list changes by
exactly one line. That is the only permitted blast radius.

## Replacement log entry

| Milestone | Crate added       | Blast radius on existing crates                          |
|-----------|-------------------|----------------------------------------------------------|
| LSP       | `ferric_lsp`      | `ferric_common`: +`keywords.rs`, +`pub mod keywords`, +`Display for Ty` |
|           |                   | `ferric_lexer`: internal keyword refactor (no public API change) |
