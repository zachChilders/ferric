//! # `proc` — Subprocess Management
//!
//! Implements the `proc` module from `docs/stdlib.md`. Provides simple capture
//! (`run`, `run_shell`), a builder-style command type (`command`, `arg`,
//! `args`, `env_set`, `env_clear`, `cwd`, `stdin_bytes`, `stdin_str`,
//! `timeout`, `spawn`), and streaming subprocess control (`spawn_streaming`,
//! `write_stdin`, `read_stdout_line`, `wait`, `kill`).
//!
//! The `$ ...` shell expression is the fire-and-forget capture; this module
//! is the full lifecycle. `builtin_shell_exec` in `lib.rs` is the runner the
//! compiler emits for `$`; the `proc_*` family lives alongside it.

// REQUIRED DEPS:
//   wait-timeout = "0.2"   (optional; for proc::timeout enforcement)

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
//     pub stdout_reader: Option<std::io::BufReader<std::process::ChildStdout>>,
// }

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::{NativeRegistry, NativeValue};
use ferric_common::Interner;

// ============================================================================
// Repr structs
// ============================================================================
//
// These mirror the variants declared in the REQUIRED NATIVE VALUE VARIANTS
// block above. They live in this file during the parallel phase so the file
// is self-contained; integration may move them up into `lib.rs` as part of
// merging the variant declarations.

pub struct ProcOutputRepr {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub struct CommandRepr {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_clear: bool,
    pub cwd: Option<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout_ms: Option<u64>,
}

impl CommandRepr {
    fn new(program: String) -> Self {
        Self {
            program,
            args: Vec::new(),
            env: Vec::new(),
            env_clear: false,
            cwd: None,
            stdin: None,
            timeout_ms: None,
        }
    }
}

pub struct ProcessRepr {
    pub child: Child,
    pub stdout_reader: Option<BufReader<ChildStdout>>,
}

// ============================================================================
// Helpers
// ============================================================================

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "expected {} argument(s), got {}",
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn expect_str(value: &NativeValue) -> Result<&String, String> {
    match value {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("expected string, got {:?}", other)),
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {:?}", other)),
    }
}

fn expect_bytes(value: &NativeValue) -> Result<&Vec<u8>, String> {
    match value {
        NativeValue::Bytes(b) => Ok(b),
        other => Err(format!("expected bytes, got {:?}", other)),
    }
}

fn expect_list(value: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match value {
        NativeValue::Array(items) => Ok(items),
        other => Err(format!("expected list, got {:?}", other)),
    }
}

fn expect_command(value: &NativeValue) -> Result<Rc<RefCell<CommandRepr>>, String> {
    match value {
        NativeValue::ProcCommand(c) => Ok(c.clone()),
        other => Err(format!("expected ProcCommand, got {:?}", other)),
    }
}

fn expect_output(value: &NativeValue) -> Result<Rc<ProcOutputRepr>, String> {
    match value {
        NativeValue::ProcOutput(o) => Ok(o.clone()),
        other => Err(format!("expected ProcOutput, got {:?}", other)),
    }
}

fn expect_process(value: &NativeValue) -> Result<Rc<RefCell<ProcessRepr>>, String> {
    match value {
        NativeValue::ProcProcess(p) => Ok(p.clone()),
        other => Err(format!("expected ProcProcess, got {:?}", other)),
    }
}

/// Classify a `std::io::Error` from a spawn into a stable Ferric-style error
/// string. The integration step will wrap these into `ProcError` variants
/// once the error enum is wired through `NativeValue`.
fn classify_spawn_error(err: std::io::Error, program: &str) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => format!("ProcError::NotFound: {}", program),
        std::io::ErrorKind::PermissionDenied => {
            format!("ProcError::PermissionDenied: {}", program)
        }
        _ => format!("ProcError::Other: {}", err),
    }
}

/// Build a `std::process::Command` from a `CommandRepr`.
fn build_std_command(repr: &CommandRepr) -> Command {
    let mut cmd = Command::new(&repr.program);
    cmd.args(&repr.args);
    if repr.env_clear {
        cmd.env_clear();
    }
    for (k, v) in &repr.env {
        cmd.env(k, v);
    }
    if let Some(dir) = &repr.cwd {
        cmd.current_dir(dir);
    }
    cmd
}

/// Naive whitespace split; quoted args are not handled. For shell-style
/// parsing, callers should use `proc::run_shell`.
fn naive_split(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(|s| s.to_string()).collect()
}

/// Execute a fully-built command, optionally piping stdin and enforcing a
/// timeout. Returns a `ProcOutputRepr` on completion. On timeout the child
/// is killed and an error string is returned.
fn run_to_completion(repr: &CommandRepr) -> Result<ProcOutputRepr, String> {
    let mut cmd = build_std_command(repr);

    // Always pipe stdout/stderr so we can capture them. Pipe stdin only if
    // we have data to send.
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if repr.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| classify_spawn_error(e, &repr.program))?;

    if let Some(data) = &repr.stdin {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(data)
                .map_err(|e| format!("ProcError::Other: writing stdin: {}", e))?;
            // Closing happens when `stdin` drops here.
        }
    }

    if let Some(ms) = repr.timeout_ms {
        // Busy-poll with try_wait. Avoids the optional `wait-timeout` dep
        // requirement so this file works whether or not it lands.
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        // Kill, then reap to avoid a zombie. Ignore kill
                        // errors (process may have just exited).
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!("ProcError::Timeout: {} ms", ms));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("ProcError::Other: {}", e));
                }
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("ProcError::Other: {}", e))?;

    Ok(ProcOutputRepr {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

// ============================================================================
// Simple capture
// ============================================================================

/// `proc::run(cmd)` — naive whitespace split; runs the first token as the
/// program with the rest as args. Quoted arguments are NOT handled. For
/// shell-style parsing, use `run_shell`.
fn builtin_proc_run(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let cmd = expect_str(&args[0])?;
    let parts = naive_split(cmd);
    if parts.is_empty() {
        return Err("ProcError::Other: empty command".to_string());
    }
    let mut repr = CommandRepr::new(parts[0].clone());
    repr.args = parts[1..].to_vec();
    let out = run_to_completion(&repr)?;
    Ok(NativeValue::ProcOutput(Rc::new(out)))
}

/// `proc::run_shell(cmd)` — runs through the system shell.
fn builtin_proc_run_shell(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let cmd = expect_str(&args[0])?;
    let (program, shell_arg) = if cfg!(windows) {
        ("cmd".to_string(), "/C".to_string())
    } else {
        ("sh".to_string(), "-c".to_string())
    };
    let mut repr = CommandRepr::new(program);
    repr.args = vec![shell_arg, cmd.clone()];
    let out = run_to_completion(&repr)?;
    Ok(NativeValue::ProcOutput(Rc::new(out)))
}

// ============================================================================
// Output inspection
// ============================================================================

fn builtin_proc_stdout(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let out = expect_output(&args[0])?;
    Ok(NativeValue::Str(out.stdout.clone()))
}

fn builtin_proc_stderr(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let out = expect_output(&args[0])?;
    Ok(NativeValue::Str(out.stderr.clone()))
}

fn builtin_proc_exit_code(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let out = expect_output(&args[0])?;
    Ok(NativeValue::Int(out.exit_code as i64))
}

fn builtin_proc_ok(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let out = expect_output(&args[0])?;
    Ok(NativeValue::Bool(out.exit_code == 0))
}

// ============================================================================
// Command builder
// ============================================================================

fn builtin_proc_command(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let program = expect_str(&args[0])?.clone();
    Ok(NativeValue::ProcCommand(Rc::new(RefCell::new(
        CommandRepr::new(program),
    ))))
}

fn builtin_proc_arg(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let cmd = expect_command(&args[0])?;
    let a = expect_str(&args[1])?.clone();
    cmd.borrow_mut().args.push(a);
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_args(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let cmd = expect_command(&args[0])?;
    let list = expect_list(&args[1])?;
    let mut new_args = Vec::with_capacity(list.len());
    for item in list {
        new_args.push(expect_str(item)?.clone());
    }
    cmd.borrow_mut().args.extend(new_args);
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_env_set(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let cmd = expect_command(&args[0])?;
    let key = expect_str(&args[1])?.clone();
    let val = expect_str(&args[2])?.clone();
    cmd.borrow_mut().env.push((key, val));
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_env_clear(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let cmd = expect_command(&args[0])?;
    cmd.borrow_mut().env_clear = true;
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_cwd(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let cmd = expect_command(&args[0])?;
    let path = expect_str(&args[1])?.clone();
    cmd.borrow_mut().cwd = Some(path);
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_stdin_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let cmd = expect_command(&args[0])?;
    let data = expect_bytes(&args[1])?.clone();
    cmd.borrow_mut().stdin = Some(data);
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_stdin_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let cmd = expect_command(&args[0])?;
    let data = expect_str(&args[1])?.clone();
    cmd.borrow_mut().stdin = Some(data.into_bytes());
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_timeout(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let cmd = expect_command(&args[0])?;
    let ms = expect_int(&args[1])?;
    if ms < 0 {
        return Err("ProcError::Other: timeout must be non-negative".to_string());
    }
    cmd.borrow_mut().timeout_ms = Some(ms as u64);
    Ok(NativeValue::ProcCommand(cmd))
}

fn builtin_proc_spawn(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let cmd = expect_command(&args[0])?;
    let repr = cmd.borrow();
    let out = run_to_completion(&repr)?;
    Ok(NativeValue::ProcOutput(Rc::new(out)))
}

// ============================================================================
// Streaming subprocess
// ============================================================================

fn builtin_proc_spawn_streaming(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let cmd = expect_command(&args[0])?;
    let repr = cmd.borrow();

    let mut std_cmd = build_std_command(&repr);
    std_cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());

    let mut child = std_cmd
        .spawn()
        .map_err(|e| classify_spawn_error(e, &repr.program))?;

    // If the builder queued stdin data, push it before handing back the
    // process. Subsequent `write_stdin` calls append more data.
    if let Some(data) = &repr.stdin {
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(data)
                .map_err(|e| format!("ProcError::Other: writing stdin: {}", e))?;
        }
    }

    let stdout_reader = child.stdout.take().map(BufReader::new);

    Ok(NativeValue::ProcProcess(Rc::new(RefCell::new(
        ProcessRepr {
            child,
            stdout_reader,
        },
    ))))
}

fn builtin_proc_write_stdin(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let proc = expect_process(&args[0])?;
    let data = expect_bytes(&args[1])?.clone();
    let mut p = proc.borrow_mut();
    let stdin = p
        .child
        .stdin
        .as_mut()
        .ok_or_else(|| "ProcError::Other: stdin not piped".to_string())?;
    stdin
        .write_all(&data)
        .map_err(|e| format!("ProcError::Other: {}", e))?;
    Ok(NativeValue::Unit)
}

fn builtin_proc_read_stdout_line(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let proc = expect_process(&args[0])?;
    let mut p = proc.borrow_mut();
    let reader = p
        .stdout_reader
        .as_mut()
        .ok_or_else(|| "ProcError::Other: stdout not piped".to_string())?;
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .map_err(|e| format!("ProcError::Other: {}", e))?;
    if n == 0 {
        return Err("ProcError::Other: EOF".to_string());
    }
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(NativeValue::Str(line))
}

fn builtin_proc_wait(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let proc = expect_process(&args[0])?;
    let mut p = proc.borrow_mut();
    let status = p
        .child
        .wait()
        .map_err(|e| format!("ProcError::Other: {}", e))?;
    Ok(NativeValue::Int(status.code().unwrap_or(-1) as i64))
}

fn builtin_proc_kill(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let proc = expect_process(&args[0])?;
    let mut p = proc.borrow_mut();
    // `kill` returns InvalidInput if the process already exited; treat that
    // as success — the caller's intent ("make sure it's dead") is satisfied.
    match p.child.kill() {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {}
        Err(e) => return Err(format!("ProcError::Other: {}", e)),
    }
    // Reap to avoid a zombie. Ignore the result: if kill was a no-op
    // because the child already exited, wait may have already been called.
    let _ = p.child.wait();
    Ok(NativeValue::Unit)
}

// ============================================================================
// Registration
// ============================================================================

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    // Simple capture
    registry.register(interner.intern("proc_run"), builtin_proc_run);
    registry.register(interner.intern("proc_run_shell"), builtin_proc_run_shell);

    // Output inspection
    registry.register(interner.intern("proc_stdout"), builtin_proc_stdout);
    registry.register(interner.intern("proc_stderr"), builtin_proc_stderr);
    registry.register(interner.intern("proc_exit_code"), builtin_proc_exit_code);
    registry.register(interner.intern("proc_ok"), builtin_proc_ok);

    // Command builder
    registry.register(interner.intern("proc_command"), builtin_proc_command);
    registry.register(interner.intern("proc_arg"), builtin_proc_arg);
    registry.register(interner.intern("proc_args"), builtin_proc_args);
    registry.register(interner.intern("proc_env_set"), builtin_proc_env_set);
    registry.register(interner.intern("proc_env_clear"), builtin_proc_env_clear);
    registry.register(interner.intern("proc_cwd"), builtin_proc_cwd);
    registry.register(interner.intern("proc_stdin_bytes"), builtin_proc_stdin_bytes);
    registry.register(interner.intern("proc_stdin_str"), builtin_proc_stdin_str);
    registry.register(interner.intern("proc_timeout"), builtin_proc_timeout);
    registry.register(interner.intern("proc_spawn"), builtin_proc_spawn);

    // Streaming
    registry.register(
        interner.intern("proc_spawn_streaming"),
        builtin_proc_spawn_streaming,
    );
    registry.register(interner.intern("proc_write_stdin"), builtin_proc_write_stdin);
    registry.register(
        interner.intern("proc_read_stdout_line"),
        builtin_proc_read_stdout_line,
    );
    registry.register(interner.intern("proc_wait"), builtin_proc_wait);
    registry.register(interner.intern("proc_kill"), builtin_proc_kill);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fresh `ProcCommand` NativeValue wrapping `program`.
    fn cmd(program: &str) -> NativeValue {
        builtin_proc_command(&[NativeValue::Str(program.to_string())]).unwrap()
    }

    /// Append an arg to a `ProcCommand`. Returns the same command (clones the
    /// `Rc` internally).
    fn arg(c: &NativeValue, a: &str) -> NativeValue {
        builtin_proc_arg(&[c.clone(), NativeValue::Str(a.to_string())]).unwrap()
    }

    /// Drop guard that always kills+waits a streaming process so leaks can't
    /// happen even if a test panics mid-way.
    struct ProcessGuard(NativeValue);
    impl Drop for ProcessGuard {
        fn drop(&mut self) {
            // Best-effort: the process may already be dead. Errors are
            // ignored on purpose.
            let _ = builtin_proc_kill(&[self.0.clone()]);
        }
    }

    fn output_of(v: &NativeValue) -> Rc<ProcOutputRepr> {
        match v {
            NativeValue::ProcOutput(o) => o.clone(),
            other => panic!("expected ProcOutput, got {:?}", other),
        }
    }

    // ---- run / run_shell ------------------------------------------------

    #[test]
    fn run_echo_hello() {
        if !cfg!(unix) {
            return;
        }
        let v = builtin_proc_run(&[NativeValue::Str("echo hello".to_string())]).unwrap();
        let out = output_of(&v);
        assert_eq!(out.stdout, "hello\n");
        assert_eq!(out.exit_code, 0);
        assert!(builtin_proc_ok(&[v]).unwrap() == NativeValue::Bool(true));
    }

    #[test]
    fn run_false_returns_nonzero() {
        if !cfg!(unix) {
            return;
        }
        let v = builtin_proc_run(&[NativeValue::Str("false".to_string())]).unwrap();
        let out = output_of(&v);
        assert_ne!(out.exit_code, 0);
        assert_eq!(builtin_proc_ok(&[v]).unwrap(), NativeValue::Bool(false));
    }

    #[test]
    fn run_shell_arithmetic() {
        if !cfg!(unix) {
            return;
        }
        let v =
            builtin_proc_run_shell(&[NativeValue::Str("echo $((1+2))".to_string())]).unwrap();
        let out = output_of(&v);
        assert_eq!(out.stdout, "3\n");
        assert_eq!(out.exit_code, 0);
    }

    // ---- output inspection ---------------------------------------------

    #[test]
    fn output_accessors() {
        if !cfg!(unix) {
            return;
        }
        let v = builtin_proc_run(&[NativeValue::Str("echo hi".to_string())]).unwrap();
        assert_eq!(
            builtin_proc_stdout(&[v.clone()]).unwrap(),
            NativeValue::Str("hi\n".to_string())
        );
        assert_eq!(
            builtin_proc_stderr(&[v.clone()]).unwrap(),
            NativeValue::Str(String::new())
        );
        assert_eq!(
            builtin_proc_exit_code(&[v.clone()]).unwrap(),
            NativeValue::Int(0)
        );
        assert_eq!(builtin_proc_ok(&[v]).unwrap(), NativeValue::Bool(true));
    }

    // ---- builder smoke -------------------------------------------------

    #[test]
    fn builder_echo_hi() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("echo");
        let c = arg(&c, "hi");
        let v = builtin_proc_spawn(&[c]).unwrap();
        let out = output_of(&v);
        assert_eq!(out.stdout, "hi\n");
        assert_eq!(out.exit_code, 0);
    }

    #[test]
    fn builder_args_appends_multiple() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("echo");
        let list = NativeValue::Array(vec![
            NativeValue::Str("a".to_string()),
            NativeValue::Str("b".to_string()),
            NativeValue::Str("c".to_string()),
        ]);
        let c = builtin_proc_args(&[c, list]).unwrap();
        let v = builtin_proc_spawn(&[c]).unwrap();
        let out = output_of(&v);
        assert_eq!(out.stdout, "a b c\n");
    }

    #[test]
    fn builder_env_set_visible_to_child() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("/bin/sh");
        let c = arg(&c, "-c");
        let c = arg(&c, "echo $FOO");
        let c = builtin_proc_env_set(&[
            c,
            NativeValue::Str("FOO".to_string()),
            NativeValue::Str("bar".to_string()),
        ])
        .unwrap();
        let v = builtin_proc_spawn(&[c]).unwrap();
        let out = output_of(&v);
        assert_eq!(out.stdout, "bar\n");
    }

    #[test]
    fn builder_env_clear_removes_inherited_vars() {
        if !cfg!(unix) {
            return;
        }
        // Set a marker in the parent; env_clear must drop it for the child.
        std::env::set_var("FERRIC_PROC_TEST_MARKER", "parent");
        let c = cmd("/bin/sh");
        let c = arg(&c, "-c");
        let c = arg(&c, "echo \"${FERRIC_PROC_TEST_MARKER:-unset}\"");
        let c = builtin_proc_env_clear(&[c]).unwrap();
        let v = builtin_proc_spawn(&[c]).unwrap();
        let out = output_of(&v);
        std::env::remove_var("FERRIC_PROC_TEST_MARKER");
        assert_eq!(out.stdout, "unset\n");
    }

    #[test]
    fn builder_cwd_changes_working_directory() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("pwd");
        let c = builtin_proc_cwd(&[c, NativeValue::Str("/tmp".to_string())]).unwrap();
        let v = builtin_proc_spawn(&[c]).unwrap();
        let out = output_of(&v);
        let trimmed = out.stdout.trim_end();
        // macOS reports `/private/tmp` for `/tmp`; accept either.
        assert!(
            trimmed == "/tmp" || trimmed == "/private/tmp",
            "expected /tmp or /private/tmp, got {:?}",
            trimmed
        );
    }

    #[test]
    fn builder_stdin_str_pipes_to_cat() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("cat");
        let c =
            builtin_proc_stdin_str(&[c, NativeValue::Str("hello".to_string())]).unwrap();
        let v = builtin_proc_spawn(&[c]).unwrap();
        let out = output_of(&v);
        assert_eq!(out.stdout, "hello");
    }

    #[test]
    fn builder_stdin_bytes_pipes_to_cat() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("cat");
        let c = builtin_proc_stdin_bytes(&[
            c,
            NativeValue::Bytes(b"raw-bytes".to_vec()),
        ])
        .unwrap();
        let v = builtin_proc_spawn(&[c]).unwrap();
        let out = output_of(&v);
        assert_eq!(out.stdout, "raw-bytes");
    }

    #[test]
    fn builder_timeout_kills_long_running() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("sleep");
        let c = arg(&c, "5");
        let c = builtin_proc_timeout(&[c, NativeValue::Int(50)]).unwrap();
        let result = builtin_proc_spawn(&[c]);
        assert!(result.is_err(), "expected timeout error, got {:?}", result);
        assert!(
            result.as_ref().unwrap_err().contains("Timeout"),
            "expected Timeout error, got {:?}",
            result
        );
    }

    // ---- streaming -----------------------------------------------------

    #[test]
    fn streaming_yes_read_three_lines() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("yes");
        let c = arg(&c, "hi");
        let p = builtin_proc_spawn_streaming(&[c]).unwrap();
        // Guard ensures the child is killed even on assertion failure.
        let guard = ProcessGuard(p.clone());

        for _ in 0..3 {
            let line = builtin_proc_read_stdout_line(&[p.clone()]).unwrap();
            assert_eq!(line, NativeValue::Str("hi".to_string()));
        }

        // Explicit kill so we can check return value; guard's drop is then
        // a no-op (kill on already-dead process is swallowed).
        let killed = builtin_proc_kill(&[p]).unwrap();
        assert_eq!(killed, NativeValue::Unit);
        drop(guard);
    }

    #[test]
    fn streaming_write_then_read() {
        if !cfg!(unix) {
            return;
        }
        let c = cmd("cat");
        let p = builtin_proc_spawn_streaming(&[c]).unwrap();
        let guard = ProcessGuard(p.clone());

        builtin_proc_write_stdin(&[
            p.clone(),
            NativeValue::Bytes(b"line one\n".to_vec()),
        ])
        .unwrap();

        let line = builtin_proc_read_stdout_line(&[p.clone()]).unwrap();
        assert_eq!(line, NativeValue::Str("line one".to_string()));

        // Done — kill (cat would otherwise block on stdin forever).
        let _ = builtin_proc_kill(&[p]);
        drop(guard);
    }

    #[test]
    fn streaming_wait_returns_exit_code() {
        if !cfg!(unix) {
            return;
        }
        // `true` exits immediately with code 0.
        let c = cmd("true");
        let p = builtin_proc_spawn_streaming(&[c]).unwrap();
        let code = builtin_proc_wait(&[p]).unwrap();
        assert_eq!(code, NativeValue::Int(0));
    }

    // ---- error paths ---------------------------------------------------

    #[test]
    fn spawn_nonexistent_binary() {
        let c = cmd("ferric_definitely_not_a_real_binary_xyz");
        let result = builtin_proc_spawn(&[c]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("NotFound"),
            "expected NotFound error, got {:?}",
            err
        );
    }

    #[test]
    fn run_empty_command_errors() {
        let result = builtin_proc_run(&[NativeValue::Str("   ".to_string())]);
        assert!(result.is_err());
    }

    #[test]
    fn timeout_negative_rejected() {
        let c = cmd("echo");
        let result = builtin_proc_timeout(&[c, NativeValue::Int(-1)]);
        assert!(result.is_err());
    }

    // ---- registration --------------------------------------------------

    #[test]
    fn register_populates_all_proc_functions() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        let names = [
            "proc_run",
            "proc_run_shell",
            "proc_stdout",
            "proc_stderr",
            "proc_exit_code",
            "proc_ok",
            "proc_command",
            "proc_arg",
            "proc_args",
            "proc_env_set",
            "proc_env_clear",
            "proc_cwd",
            "proc_stdin_bytes",
            "proc_stdin_str",
            "proc_timeout",
            "proc_spawn",
            "proc_spawn_streaming",
            "proc_write_stdin",
            "proc_read_stdout_line",
            "proc_wait",
            "proc_kill",
        ];
        for name in names {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "{} not registered",
                name
            );
        }
    }
}
