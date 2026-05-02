# ferric_diagnostics

Renders compiler and runtime errors as human-readable, rustc-style diagnostics with source context and span pointers.

## Entry point

```rust
let renderer = Renderer::with_interner(source.clone(), &interner);

renderer.render_lex_error(&err)
renderer.render_parse_error(&err)
renderer.render_resolve_error(&err)
renderer.render_type_error(&err)
renderer.render_exhaustiveness_error(&err)
renderer.render_runtime_error(&err)
```

Use `Renderer::new(source)` when no interner is available; symbol IDs are printed as raw integers in that case.

## Output format

Each rendered diagnostic includes:

- An `error:` header with a short description
- A `file:line:col` location line
- The relevant source line(s) from the original input
- Caret (`^`) underlines pointing at the exact span
- Secondary labels for related spans (e.g. the previous definition in a duplicate-definition error)

Example:

```
error: undefined variable `x`
  --> test.fe:3:5
   |
 3 |     x + 1
   |     ^ not found in this scope
```

## Design

`Renderer` takes a snapshot of the source string at construction time and converts byte `Span` offsets to line/column numbers on demand. All six render methods follow the same pattern, so adding a new error type means adding one method.
