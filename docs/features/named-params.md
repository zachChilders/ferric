# Named-Argument Calling Convention

Every call site in Ferric uses named arguments. This includes user functions, stdlib natives, async intrinsics, and method calls. The resolver canonicalises arguments to definition order before the type checker or VM see them, so positional drift is impossible.

## Syntax

```rust
fn add(a: Int, b: Int) -> Int { a + b }

let r1 = add(a: 3, b: 4)        // definition order
let r2 = add(b: 10, a: 5)       // out of order — resolver canonicalises
```

Method calls use the same shape:

```rust
val.describe()                  // zero-arg
"hello".len()                   // (only if `len` is a trait method)
http::get(url: "https://...")   // module-qualified call
```

Stdlib natives publish their parameter lists at registration time so the same convention applies:

```rust
println(s: "hello")
str_split(s: "a,b,c", sep: ",")
spawn(task: my_async_fn())
join(a: h1, b: h2)
```

## Default parameters

Trailing parameters can declare defaults:

```rust
fn greet(name: Str, greeting: Str = "Hello") -> Str {
    greeting + ", " + name + "!"
}

greet(name: "world")                            // "Hello, world!"
greet(name: "Ferric", greeting: "Hey")          // "Hey, Ferric!"
greet(greeting: "Hi", name: "there")            // "Hi, there!" — order doesn't matter
```

Defaults are expressions evaluated at the call site each time, not at definition time.

## Rules

- Every argument must be named at the call site. `add(3, 4)` is a parse error.
- An argument name must match a declared parameter. Unknown names are a `ResolveError::UnknownNamedArg`.
- Each parameter must be bound exactly once per call.
- Required parameters (no default) must be supplied; `MissingNamedArg` otherwise.
- Default values may reference earlier parameters? No — defaults are independent expressions, evaluated in their own scope.

## Why required-named-everywhere

The motivation is **call-site readability** and **safety in the face of refactors**:

- `str_split(s: haystack, on: ",")` reads like a sentence.
- Reordering parameters in a fn definition cannot silently swap arguments at call sites — the names anchor each value.
- Stdlib functions with many parameters (`str_pad_start(s, to, with)`, `http::get(url, headers, timeout)`) self-document at the call site.

## Implementation note

The resolver builds a `(fn_name, param_names)` table from:

- `NativeRegistry::fn_table()` — published by `register_named` calls in stdlib modules.
- `async_intrinsic_param_table()` — for `spawn`, `join`, `sleep`, `block_on`, `shell_run_async`.
- User function definitions in the AST.

For each `Expr::Call`, the resolver looks up the callee, matches each `NamedArg` to its declared position, and writes a canonical-order argument list back into the AST. The type checker and VM only ever see the canonical order — they don't know names.

This means:

- `register` (without params) is reserved for compiler-internal natives that the user can't call by name (e.g. `__shell_exec`).
- A new stdlib function added through `register_named` automatically becomes callable with named args; nothing else needs to change.

## See also

- [`../adding-syntax.md`](../adding-syntax.md) — Section H covers adding new natives the right way.
- [`traits.md`](traits.md) — methods follow the same convention; `self` is the receiver.
