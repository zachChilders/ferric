# Shell-First Execution

Ferric's defining feature. The `$` sigil drops into a shell from inside any expression, runs a command, and returns a `ShellOutput` value.

```rust
let out = $ echo hello
println(s: shell_stdout(output: out))
println(s: int_to_str(n: shell_exit_code(output: out)))
```

## Syntax

```
$ <command-line>
```

The command extends to the end of the line. Multi-line commands use a trailing `\`:

```rust
let log = $ echo 1 \
            2 \
            3
```

Literal `{`, `}`, `$`, and pipes pass through verbatim — the shell, not Ferric, parses them:

```rust
let first = $ echo a b c | awk '{print $1}'
```

## Interpolation: `@{expr}`

`@{...}` evaluates a Ferric expression and substitutes its string form into the command:

```rust
let name = "world"
let greet = $ echo Hello, @{name}!

let a = 3
let b = 4
let r = $ echo @{a + b}
```

`Int` interpolations are coerced via `int_to_str` automatically. Other types must already be `Str`.

`@{...}` is the **only** interpolation form. Plain `$VAR` goes to the shell unchanged.

## The return value: `ShellOutput`

A shell expression evaluates to a `ShellOutput { stdout: Str, exit_code: Int }`. Field access is via stdlib functions, not `.stdout`:

```rust
let result = $ ls
let text = shell_stdout(output: result)
let code = shell_exit_code(output: result)
```

This is intentional: swallowing the exit code is not the default. If you want fire-and-forget, redirect or assign-and-ignore explicitly.

## Lowering

The compiler lowers `$ <parts>` to:

1. Push each part on the stack — string literals as `Str`, `Int` interpolations through `int_to_str`, others as-is.
2. `Concat` them into a single command string.
3. Call the compiler-internal `__shell_exec` native, which runs the command and returns a `ShellOutput`.

The native lives in `crates/ferric_stdlib/src/lib.rs` (`builtin_shell_exec`); `shell_stdout` / `shell_exit_code` are public natives that the user calls.

## Inside `async` blocks

When an `async fn` or `async { ... }` contains `$`, the lowering pass rewrites it to `await shell_run_async(cmd: ...)` so it doesn't block the worker thread. From a user's perspective the syntax is unchanged.

If a program uses `async` *anywhere* and also has shell commands at sync top-level, the compiler emits an `AsyncWarning::BlockingShell` — those commands still block. In a pure sync program (no `async` syntax) the warning is suppressed as noise.

See [`async.md`](async.md).

## Errors

- A malformed `@{...}` (unclosed brace, parse error inside) is a `ParseError` with a span pointing at the interpolation.
- Runtime command failures (non-zero exit, missing binary) do **not** abort the program — they appear in `exit_code`. If you want abort-on-failure, wrap in `require`:

  ```rust
  let out = $ ls /missing
  require shell_exit_code(output: out) == 0, "ls failed"
  ```

## See also

- [`require.md`](require.md) — combining `$` with `require` for idempotent host enforcement.
- [`async.md`](async.md) — `shell_run_async` and `BlockingShell` warnings.
