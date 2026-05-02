# Task: Stdlib — `proc`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `proc` module from `docs/stdlib.md` (section "`proc` —
Subprocess Management"). The `$ ...` shell expression is fire-and-forget
capture; `proc` is the full lifecycle.

## Output File

- `crates/ferric_stdlib/src/proc_.rs` (file is `proc_` to avoid the
  reserved-word conflict).

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                                       // shared
//   ProcOutput(Rc<ProcOutputRepr>)
//   ProcCommand(Rc<RefCell<CommandRepr>>)
//   ProcProcess(Rc<RefCell<ProcessRepr>>)
//
// pub struct ProcOutputRepr {
//     pub stdout: String,
//     pub stderr: String,
//     pub exit_code: i32,
// }
//
// pub struct CommandRepr {
//     pub program: String,
//     pub args: Vec<String>,
//     pub env: Vec<(String, String)>,
//     pub env_clear: bool,
//     pub cwd: Option<String>,
//     pub stdin: Option<Vec<u8>>,
//     pub timeout_ms: Option<u64>,
// }
//
// pub struct ProcessRepr {
//     pub child: std::process::Child,
//     pub stdout_reader: Option<BufReader<...>>,
// }
```

## Required Deps

None — `std::process` is sufficient. Optional: `wait-timeout = "0.2"` if
you want timeout support without a custom poll loop.

```rust
// REQUIRED DEPS:
//   wait-timeout = "0.2"   (optional; for proc::timeout enforcement)
```

## Function Surface (must register)

Simple capture:

`run`, `run_shell`.

Output inspection:

`stdout`, `stderr`, `exit_code`, `ok`.

Command builder:

`command`, `arg`, `args`, `env_set`, `env_clear`, `cwd`, `stdin_bytes`,
`stdin_str`, `timeout`, `spawn`.

Streaming:

`spawn_streaming`, `write_stdin`, `read_stdout_line`, `wait`, `kill`.

## Implementation Notes

- `run(cmd)` splits `cmd` on whitespace (simple) and runs the first token
  as the program with the rest as args. **Document this is a naive split**
  — quoted arguments are not handled. For shell-style parsing, use
  `run_shell`.
- `run_shell(cmd)` invokes `sh -c <cmd>` on Unix, `cmd /C <cmd>` on
  Windows.
- `command(program)` returns a fresh `Command` builder.
- `arg`/`args` mutate and return the same handle.
- `env_clear(c)` sets a flag; `env_set` after `env_clear` adds to the
  cleared environment.
- `stdin_bytes`/`stdin_str` queue bytes to write to the child's stdin; the
  spawn step pipes stdin and writes them.
- `timeout(c, ms)` records a deadline; `spawn` enforces it (`wait_timeout`
  if available; otherwise busy-poll with `try_wait`).
- `spawn(c)` runs to completion. On success returns
  `ProcOutput`. On timeout, kills the child and returns `ProcError::Timeout`.
- `spawn_streaming(c)` returns a `Process` handle whose stdout is buffered
  for line-reads.
- `read_stdout_line(p)` reads one line (without trailing newline). EOF
  returns `Err`.
- `wait(p)` blocks until exit, returns the exit code.
- `kill(p)` sends SIGKILL (or `TerminateProcess` on Windows).

## Tests

Use commands available on most systems:

- `run("echo hello")` → `ok=true`, `stdout="hello\n"`, `exit_code=0`.
- `run("false")` → `ok=false`, `exit_code=1`.
- `run_shell("echo $((1+2))")` → `stdout="3\n"`.
- Builder smoke:
  - `c = command("echo"); arg(c, "hi"); spawn(c).stdout` → `"hi\n"`.
- `args` adds multiple.
- `env_set` / `env_clear`:
  - `command("/bin/sh"); arg(_, "-c"); arg(_, "echo $FOO"); env_set("FOO",
    "bar")`. Spawn → stdout `"bar\n"`.
- `cwd`:
  - `command("pwd"); cwd("/tmp"); spawn` → stdout starts with `/tmp` (or
    `/private/tmp` on macOS — accept both).
- `stdin_str`:
  - `command("cat"); stdin_str("hello"); spawn` → `stdout = "hello"`.
- `timeout`:
  - `command("sleep"); arg("5"); timeout(50); spawn` → `Err(ProcError::Timeout)`.
- `spawn_streaming("yes hi")`, then `read_stdout_line` ×3 returns `"hi"`,
  then `kill(p)`. Important: kill the child to stop the test.
- `command("nonexistent_binary"); spawn` → `Err(ProcError::NotFound)`.

Aim for ~15 tests.

**Test hygiene:** every spawned child must be killed or waited within the
test. Wrap in helpers if necessary.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Tests must work on both macOS and Linux. Use `cfg!(unix)` to gate
  shell-specific tests.

## Acceptance

- One file with the listed register coverage.
- Tests cover capture, builder, env, cwd, stdin, timeout, streaming.
- No leaked child processes.
