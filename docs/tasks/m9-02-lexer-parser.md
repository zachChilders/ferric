# M9 — Task 2: Lexer + Parser

> Adds the lexer and parser support for the new effects syntax, plus the parse-time
> desugaring of `async fn` / `.await` / `gen fn` / `yield` into the new
> `PerformExpr` / `Ty::Eff` form.
>
> **Prerequisite:** [Task 1](m9-01-common-types.md) must be complete — every AST
> node and `Ty` variant this task constructs lives in `ferric_common`.
>
> See [m9-00-overview.md](m9-00-overview.md) for milestone-level design
> decisions and pipeline shape.

---

## What this task does

Teaches the lexer to recognise the new effects keywords, teaches the parser to
build `EffectDecl`, `PerformExpr`, `HandleExpr`, and `ResumeExpr` nodes, and
rewrites `async fn` / `.await` / `gen fn` / `yield` at parse time so that no
async-specific AST nodes survive into later stages.

After this task ships, `Item::AsyncFn`, `Expr::Await`, and `Expr::AsyncBlock`
are still **defined** in `ferric_common` (they get deleted in Task 7) but are
**no longer constructed** by the parser — every async/await form is rewritten to
the effects form before parsing returns.

---

## New keywords

Add to `ferric_common::keywords::KEYWORDS`:

```
effect  perform  handle  with  resume  gen  yield
```

`async`, `await` remain as keywords but are now syntactic sugar handled in the
parser, not semantic constructs tracked through later stages.

---

## Lexer changes

- `effect`, `perform`, `handle`, `with`, `resume`, `gen`, `yield` — emit as
  keyword tokens.
- `async`, `await` — already tokenized; no change to the lexer.
- No other lexer changes.

---

## Parser changes

### `effect` declarations (new item form)

```
effect Name<TypeParams> {
    OpName(param: Type, ...) -> ResumeType,
    ...
}
```

Parsed into `Item::EffectDecl`. Effect declarations may only appear at module
scope, not inside function bodies. Emit `ParseError::EffectDeclInsideFn` if
found inside a body.

### `perform` expressions

```
perform Effect::Op(name: expr, ...)
```

Parsed into `Expr::Perform`. Precedence: same as a function call (high). Args
are named — they match the parameter names in the corresponding `EffectOp`
declaration (canonicalised by name in Task 3).

### `handle...with` expressions

```
handle { expr } with {
    Effect::Op(param, ...) => body,
    ...
}
```

Parsed into `Expr::Handle`. Multiple clauses are allowed; each must name a
distinct `Effect::Op` pair. The continuation variable `k` is implicitly bound in
every handler body; users may shadow it by naming a param `k` (emit
`ParseWarning::ShadowsContinuation`).

### `resume` expressions

```
resume k with expr
```

Parsed into `Expr::Resume`. `k` must be the continuation variable of the
enclosing handler clause; the parser tracks this statically and emits
`ParseError::ResumeOutsideHandler` if `resume` appears elsewhere.

### `async fn` desugaring (parse time)

When the parser sees `async fn name(params) -> T { body }`:

1. Rewrite the return type to `Ty::Eff(vec![EffectRef { name: "Async", args: [] }], T)`.
2. Wrap the body: replace every `expr.await` with
   `Expr::Perform(PerformExpr { effect: "Async", op: "Suspend", args: [("future", expr)] })`.
3. Emit a standard `Item::Fn` — no `Item::AsyncFn`.

`async { block }` desugars to an immediately-invoked `fn` wrapped in a handler.

### `gen fn` / `yield` desugaring (parse time)

When the parser sees `gen fn name(params) -> T { body }`:

1. Rewrite the return type to
   `Ty::Eff(vec![EffectRef { name: "Gen", args: [T] }], Ty::Unit)`.
2. Replace every `yield expr` with
   `Expr::Perform(PerformExpr { effect: "Gen", op: "Yield", args: [("value", expr)] })`.
3. Emit a standard `Item::Fn`.

---

## New parse-time errors and warnings

```rust
// Add to ParseError:
ParseError::EffectDeclInsideFn   { span: Span },
ParseError::ResumeOutsideHandler { span: Span },

// Add to ParseWarning:
ParseWarning::ShadowsContinuation { span: Span },
```

All variants carry `Span` (Rule 5).

---

## Done when

- [ ] Lexer emits keyword tokens for `effect`, `perform`, `handle`, `with`, `resume`, `gen`, `yield`
- [ ] Parser builds `Item::EffectDecl` from `effect Name { ... }` syntax
- [ ] Parser rejects `effect` declarations inside function bodies with `ParseError::EffectDeclInsideFn`
- [ ] Parser builds `Expr::Perform` from `perform Effect::Op(name: expr, ...)`
- [ ] Parser builds `Expr::Handle` from `handle { ... } with { ... }`
- [ ] Parser builds `Expr::Resume` from `resume k with expr`
- [ ] Parser rejects `resume` outside a handler clause with `ParseError::ResumeOutsideHandler`
- [ ] Parser warns on a clause param named `k` with `ParseWarning::ShadowsContinuation`
- [ ] `async fn name(...) -> T { body }` is rewritten to a plain `Item::Fn` returning `Ty::Eff([Async], T)` with `.await` → `perform Async::Suspend`
- [ ] `gen fn name(...) -> T { body }` is rewritten to a plain `Item::Fn` returning `Ty::Eff([Gen<T>], Unit)` with `yield` → `perform Gen::Yield`
- [ ] No `Item::AsyncFn`, `Expr::Await`, or `Expr::AsyncBlock` nodes appear in the parser's output (verified by a `--dump-ast` test on M8 async fixtures)
- [ ] All M1–M8 parse fixtures continue to parse — desugaring is invisible at the source level
- [ ] All new error/warning variants carry `Span` (Rule 5)
