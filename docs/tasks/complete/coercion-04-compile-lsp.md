# Coercion — Task 4: Compiler Emission + LSP Inlay Hints

> Compiler reads `TypeResult::coercions` and emits the `to()` call before
> pushing the argument. LSP inlay hint pass surfaces every coerced argument
> as `as T` so the implicit coercion is visible in the editor.
>
> **Prerequisite:** [Task 3](coercion-03-typecheck.md) must be complete.
>
> See [coercion-00-overview.md](coercion-00-overview.md) for design decisions.

---

## What this task does

End-to-end wiring. After Task 3, the type checker records coercions but
the compiler ignores them, so coerced calls fail at runtime. This task
makes the compiler emit the `to()` call and the LSP show the coercion as
an inlay hint. After this task, the feature is complete and shippable.

---

## Compiler changes (`ferric_compiler`)

When compiling a `CallExpr`, for each argument `i`:

1. Look up `arg.node_id` in `TypeResult::coercions`.
2. If present:
   - Compile the argument expression as before, leaving the source value on the stack.
   - Emit a `Call(1)` to the resolved `to` method `DefId`. The `to` impl is
     a single-argument method that consumes `self` and returns the converted
     value.
   - The resulting `Float` / `Str` / etc. is now the value pushed for the
     outer call.
3. If absent: emit the argument as before.

No new opcodes. No changes to `Op::Call` semantics. The coercion compiles
to an ordinary trait method call.

If the existing compiler resolves trait method calls via
`types.method_dispatch: HashMap<NodeId, DefId>` (M5 pattern), populate that
table with the coercion's `to` `DefId` keyed by the arg `NodeId` so the same
machinery applies. Or use a parallel path — pick whichever is cleaner.

---

## LSP inlay hints

The LSP inlay-hint pass should surface implicit coercions visibly. When a
call argument has an entry in `TypeResult::coercions`:

- Render an inlay hint at the argument's end position
- Hint text: `as T` where `T` is the target parameter type
- Use the same hint style as existing inlay hints (named-arg labels, etc.)

Example:

```ferric
let output = $ "echo hello"
print_str(s: output)
              ^^^^^^ as Str
```

The hint keeps the "no surprises" contract — the coercion is implicit in
source but visible in the editor.

If `crates/ferric_lsp/src/handlers/inlay_hints.rs` (or wherever inlay hints
live) doesn't yet expose `TypeResult` to the hint pass, plumb it through.
The pipeline snapshot (`crates/ferric_lsp/src/pipeline.rs`) already has the
`TypeResult` from typecheck — the hint handler just needs to read
`type_result.coercions` and walk the AST to find the corresponding argument
positions.

---

## End-to-end fixtures

Add example fixtures under `examples/`:

### `examples/coercion_int_to_float/`

```ferric
fn add_floats(a: Float, b: Float) -> Float {
    a + b
}

fn main() {
    let result = add_floats(a: 1, b: 2.5)
    println(s: float_to_str(n: result))
}
```

Expected output: `3.5\n`. Verifies `Int → Float` coercion at the `a:` argument.

### `examples/coercion_shelloutput_to_str/`

```ferric
fn echo(s: Str) {
    println(s: s)
}

fn main() {
    let output = $ "echo hello"
    echo(s: output)
}
```

Expected output: `hello\n` (or whatever the shell-output-stdout convention is).
Verifies `ShellOutput → Str` coercion.

### `examples/coercion_no_chaining/`

A negative test: a call site where two-hop coercion would be needed. Should
emit a `TypeError::TypeMismatch` and exit non-zero. Pick types where
`A → B → C` impls exist and the arg type is `A`, param type is `C`. If no
such configuration exists with built-in impls, add a user-defined impl in
the test fixture or skip this fixture and rely on a unit test.

### `examples/coercion_ambiguous/`

A negative test: a call site where two distinct `To<Float>` impls apply for
source `Int`. Likely requires user-defined impls in the fixture (the
built-ins are non-ambiguous). Expected output: `TypeError::AmbiguousCoercion`
diagnostic with both candidate locations listed.

For each fixture, capture `expected.txt` and `exit` per `CLAUDE.md`.

---

## Done when

- [ ] Compiler emits the `to()` call for every entry in `TypeResult::coercions` before pushing the argument
- [ ] No new opcodes added — coercion is an ordinary `Call(1)` to the resolved `to` method
- [ ] At least one VM-level test verifies that a coerced call produces the converted value (e.g. `Int → Float`)
- [ ] LSP inlay hint pass renders `as T` at every coerced argument position
- [ ] `examples/coercion_int_to_float/` fixture passes end-to-end
- [ ] `examples/coercion_shelloutput_to_str/` fixture passes end-to-end
- [ ] `examples/coercion_no_chaining/` (or unit test) verifies two-hop coercions emit `TypeMismatch`
- [ ] `examples/coercion_ambiguous/` (or unit test) verifies two-candidate cases emit `AmbiguousCoercion`
- [ ] `cargo build --workspace` clean
- [ ] `cargo test --workspace` passes (existing + new tests)
- [ ] `cargo clippy --workspace --all-targets` clean
- [ ] LSP test verifies the inlay hint appears at the right position with `as T` text
- [ ] All M1–M9 fixtures still pass — no behaviour change at non-coercion call sites
