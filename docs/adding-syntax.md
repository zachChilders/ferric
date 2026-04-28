# Adding New Syntax to Ferric

This is a working checklist for the touchpoints involved in introducing a new piece of syntax. The pipeline is deliberately decoupled — every stage has one public entry point and reads only the previous stage's output type — so a syntax addition is a vertical slice that visits each stage in turn rather than a single sweeping refactor.

> Read this alongside [`architecture.md`](architecture.md). The non-negotiable rules there govern what each stage may and may not see.

---

## The pipeline, one more time

```
source
  → ferric_lexer::lex
  → ferric_parser::parse_with_interner
  → ferric_manifest::load_manifest
  → ferric_resolve::resolve_with_natives_and_builtins         (initial)
  → ferric_module::resolve_modules
  → ferric_resolve::resolve_with_imports_and_builtins         (re-run)
  → ferric_traits::build_registry
  → ferric_infer::typecheck
  → ferric_exhaust::check_exhaustiveness
  → ferric_async::lower_async
  → ferric_compiler::compile
  → ferric_vm::Executor::run
```

Most syntax additions touch some subset of: lexer, parser, AST in `ferric_common`, resolver, infer, compiler, VM, diagnostics, and (when there are user-visible errors) example fixtures. A few touch the LSP, the TextMate grammar, or the AST JSON serialiser too.

---

## Decision tree — what kind of syntax is this?

Pick the closest shape; each row links to a section below.

| Adding… | Sections to read |
|---|---|
| A new keyword (`yield`, `pub`, `loop`-variant) | A, B, then whichever of D/E/F applies |
| A new operator (binary, unary, postfix) | A, B, C, D, E, F, G |
| A new expression form (e.g. a literal, control-flow shape) | B, C, D, E, F, G, plus diagnostics |
| A new statement form (sits in `Stmt`, not `Expr`) | B, C, D, E, F, G |
| A new top-level item (`Item::*`) | B, C, D, E, F, G — and check imports/exports |
| A new type form (`TypeAnnotation::*` / new `Ty` variant) | B, C, D, E, plus a sweep of `apply` / `unify` in infer |
| A new pattern form | B, C, D, E (exhaust!), F, G |
| A new bytecode op only | F, G — no surface change |
| A new native stdlib function | Section H (does **not** touch any stage) |

---

## Section A — Lexing a new keyword or operator

Files involved:

- `crates/ferric_common/src/keywords.rs` — single source of truth.
  - Add the keyword string to `KEYWORDS` *or* the operator to `OPERATORS`.
  - This list drives the TextMate grammar generation in `ferric_lsp/build.rs`. Adding a keyword here highlights it correctly in VS Code on the next build with no further action.
- `crates/ferric_common/src/tokens.rs` — `TokenKind` enum + `description()`.
  - Add a new variant (e.g. `TokenKind::Yield`).
  - Update the `description()` arm so error messages name the token correctly.
- `crates/ferric_lexer/src/lib.rs` — the keyword match table and any handwritten lookahead for multi-char operators.
  - Add the keyword/operator branch.
  - The test `keywords_match_common_list` enforces parity between the lexer and `keywords.rs` — running `cargo test -p ferric_lexer` will fail loudly if you forget.

If the new operator is multi-character (e.g. `<<`, `??`), make sure the longer match is checked **before** the shorter; the lexer is hand-rolled and order matters.

---

## Section B — AST in `ferric_common`

All AST types live in `crates/ferric_common/src/ast.rs`. Conventions:

- Every node carries `id: NodeId` and `span: Span`. Both are non-optional. (Rule 5: every error type carries a span; many of them quote node ids back, so missing them breaks downstream stages.)
- Each new variant should derive `Debug, Clone, PartialEq, Serialize, Deserialize` like its neighbours. Serde derives keep `--dump-ast` and the LSP working.

Decide which enum the new node lives on:

- New top-level construct → `Item`. Update `Item::id()` and `Item::span()` exhaustive matches.
- New expression form → `Expr`. Update `Expr::id()` and `Expr::span()` exhaustive matches.
- New statement form → `Stmt`. Update `Stmt::id()` and `Stmt::span()` exhaustive matches.
- New pattern form → `Pattern`. Update `Pattern::span()`.
- New operator → `BinOp` (and update `BinOp::precedence()`) or `UnOp`.
- New type form → `TypeAnnotation`. Most additions also need a corresponding `Ty` variant in `crates/ferric_common/src/types.rs` (see Section D).

If the AST grows new fields that wrap shell or async or anything else with concurrency implications, keep the compile-time `Send + Sync` assertion in `ast.rs` happy.

If the new node has a structural sub-shape that other stages reuse, extract a struct (see how `FnItem` / `AsyncFnItem` share fields). Match-as-decorator beats boolean flags.

---

## Section C — Parser

`crates/ferric_parser/src/lib.rs` (~3.9 kLoC). Hand-written recursive descent.

Touchpoints:

- Add a parse function (`parse_yield`, `parse_pipeline_op`, etc.) at the right precedence tier. The `BinOp::precedence()` table in `ast.rs` is the source of truth for binary precedence; respect it when adding a new operator.
- Wire the new function into the dispatch site:
  - Items: `parse_item` / `parse_top_level`.
  - Statements: `parse_stmt`.
  - Primary expressions / postfix chains: `parse_primary` and `parse_call_chain` (or whatever the current names are — check the file).
  - Type annotations: `parse_type_annotation`.
  - Patterns: `parse_pattern`.
- Set `id` to a fresh `NodeId` from the parser's counter. Set `span` from the token range you consumed.
- Add a parser test (`#[cfg(test)] mod tests`) that asserts the parsed AST shape and span. The parser is the place where bad input is caught early — also add a negative test that the parser produces a `ParseError` with a useful span if the new construct is misused.

If the new syntax reserves a brand-new `ParseError` variant (e.g. "yield outside generator"), add it in `crates/ferric_common/src/errors.rs` alongside its `Span`-bearing fields, and add a `render_parse_error` arm in `crates/ferric_diagnostics/src/lib.rs`.

---

## Section D — Name resolution

`crates/ferric_resolve/src/lib.rs` (~1.5 kLoC).

Touchpoints:

- `walk_item` / `walk_stmt` / `walk_expr` / `walk_pattern`: add a match arm so the new node is traversed. Forgetting this means downstream stages will see unresolved `Variable` nodes.
- If the new construct introduces bindings: allocate slot indices via the existing `def_slots` / `fn_slots` machinery. Mutability is tracked in the resolver — new mutable bindings need to set the appropriate flag.
- If the new construct introduces a new scope: push and pop on `scope_stack`.
- If the new construct affects loop depth (`break` / `continue` validity), update `loop_depth`. If it affects "are we inside an async context", that's the type checker's job (see Section E).
- If the new construct creates closures (anything that captures from an outer scope), the existing `closure_stack` capture-analysis code is the place to extend. The resolver records captures in `ResolveResult::captures`; the compiler reads them back and builds a `Value::Closure`.
- New resolve-time errors → add a `ResolveError` variant in `errors.rs`, plumb it through `Renderer::render_resolve_error`.

`resolve_with_natives_and_builtins` and `resolve_with_imports_and_builtins` are the two public entry points. They share most of the body; both must handle the new node.

---

## Section E — Type checking + traits

`crates/ferric_infer/src/lib.rs` (~2.7 kLoC).

Touchpoints:

- For a new expression form: add a case to the inference walker that produces a `Ty` and writes it into `node_types`. If the form is polymorphic, allocate fresh type variables and emit constraints; the existing `try_unify` is the unification engine.
- For a new type form (e.g. `Ty::Pipe<A, B>`): every walker over `Ty` must learn about it. The reliable way to find them is to grep for an existing variant (e.g. `Ty::Async`) and add a parallel arm everywhere it appears: `apply`, `occurs_raw`, `free_vars_raw`, `rename_vars`, `default_remaining_vars`, `try_unify`, plus the `description()` arm in `types.rs` and the `Display` impl.
- For a new operator: extend the binary/unary checking branches and decide which `Op::*` family it lowers to (Section F).
- For new typing errors: add `TypeError::*` in `errors.rs` and a `render_type_error` arm in diagnostics. Prefer specific variants (e.g. `AwaitOnNonAsync`) over the generic `Mismatch` when the user-facing message is meaningfully different.
- If the new construct affects "method dispatch on this type" or trait resolution, that's `crates/ferric_traits/src/lib.rs`. The trait registry is consulted from `typecheck` only; the compiler reads the resolved `method_dispatch: HashMap<NodeId, DefId>` instead.

If the new construct can fall inside a `match`, `crates/ferric_exhaust/src/lib.rs` needs to learn about it — at minimum to treat it as opaque, at most to participate in coverage analysis.

---

## Section F — Async lowering, compiler, and VM

`crates/ferric_async/src/lib.rs` is mostly identity but every recursive walker matches on every `Item` / `Expr`. If your new node lives on those enums, add the pass-through arm so it survives the lowering. If your node has subtle async semantics (e.g. it can hold an `Async<T>`), think about what `await` does inside it.

`crates/ferric_compiler/src/lib.rs` (~1.5 kLoC):

- New expression / statement: add a `compile_expr` / `compile_stmt` branch that emits the right opcode sequence.
- New opcode: define it in `crates/ferric_common/src/bytecode.rs` (the schema) **and** wire its execution in `crates/ferric_vm/src/bytecode.rs`. The schema is the source of truth — keep the VM dispatch's exhaustive match in sync.
- New constant kind (e.g. a new value pattern): extend `Constant` in the bytecode schema and the VM's constant-loading path.

`crates/ferric_vm/src/bytecode.rs` (~1.4 kLoC):

- Implement the opcode in `BytecodeVM::run_chunk_to_completion` (or whatever the current dispatch fn is named).
- If the new opcode produces a new value shape, add a `Value` variant in `crates/ferric_vm/src/lib.rs` *and* a `Value::new_*` constructor. Rule 7: only `ferric_vm` may construct `Value::Foo(..)` directly; everywhere else uses the constructor.
- If the new opcode can fail at runtime in a new way, add a `RuntimeError` variant and a `render_runtime_error` arm.
- For async-relevant additions, decide whether the opcode runs only on the main thread (uses `&mut self`) or can run on a worker (`Arc<...>` shared state). The scheduler is in `ferric_vm`; cross-thread operations go through `Scheduler::handles`.

---

## Section G — Diagnostics, examples, and tests

- `crates/ferric_diagnostics/src/lib.rs` exhaustively matches every error variant in `ferric_common::errors`. Compilation will tell you what's missing.
- Add at least one happy-path fixture under `examples/<name>/` with `test.fe`, `expected.txt`, and `exit`. `tests/examples.rs` runs the binary and diffs both. This is the canonical place — **do not** put `.fe` files in `/tmp` while iterating.
- Add at least one negative fixture if the new syntax can be misused (parse error, resolve error, type error). The expected message is captured byte-for-byte, so writing it once locks in the rendered diagnostic.
- If you added a parser-level rejection, write a `--dump-ast` smoke fixture so the AST shape is also pinned.
- Stage-internal Rust tests live in `crates/<stage>/src/lib.rs` under `#[cfg(test)] mod tests`. Use these for logic that's awkward to express end-to-end (precedence quirks, edge cases in unification).

---

## Section H — Adding a stdlib native

This is **not** a syntax change, but it's the most common neighbouring task.

- Pick the right module under `crates/ferric_stdlib/src/<module>.rs`. Each module has its own `register(registry, interner)` function that's called from `register_stdlib`.
- Use `registry.register_named(interner, "name", &["param1", "param2"], func)`. The `register_named` form publishes the parameter list to the resolver, which is what enables the named-argument calling convention. The bare `register` form is reserved for compiler-internal natives like `__shell_exec`.
- If the function needs scheduler access (spawn / join / await machinery), it is *not* a normal native — it's an async intrinsic dispatched directly inside `Op::Call` in the VM. Register only its parameter table via `async_intrinsic_param_table`. See the existing `spawn` / `join` / `block_on` for the pattern.
- LSP completion picks up new natives automatically via `ferric_lsp/src/stdlib_names.rs` if you regenerate or extend that table. Check `stdlib_names.rs` after adding.

No stage signatures change. No bytecode changes. Just the registration function and (optionally) an example fixture.

---

## Quick checklist (for muscle memory)

For a typical new expression form:

1. `keywords.rs` (if it introduces a keyword) — add to `KEYWORDS`.
2. `tokens.rs` — `TokenKind::Foo` + `description()`.
3. `ferric_lexer` — recognise the token; ensure `keywords_match_common_list` still passes.
4. `ast.rs` — `Expr::Foo { id, span, ... }`; update `Expr::id()` / `Expr::span()`.
5. `ferric_parser` — `parse_foo`; wire into the right dispatch site; add tests.
6. `ferric_resolve` — walker arm; bindings/scoping if applicable.
7. `ferric_infer` — inference arm writing into `node_types`; new `TypeError`s if needed.
8. `ferric_exhaust` — pass-through arm if it can appear in `match`.
9. `ferric_async` — pass-through walker arm.
10. `ferric_compiler` — emit opcodes.
11. `ferric_common::bytecode` + `ferric_vm` — new opcode(s) and execution.
12. `ferric_diagnostics` — render any new error variants.
13. `examples/<name>/` — happy path and negative cases.
14. `cargo test` — green across the workspace.

If any one of these is omitted, the most common failure mode is "the parser sees it but the resolver / compiler panics on an unhandled match arm". Walking the list end-to-end is faster than chasing a panic backwards.
