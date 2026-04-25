//! End-to-end example regression test.
//!
//! Each subdirectory under `examples/` is a self-contained fixture:
//!
//! ```text
//! examples/<name>/
//!     <name>.fe       — the source program
//!     expected.txt    — combined stdout+stderr the program must produce
//!     exit            — the integer exit code (e.g. `0\n`)
//! ```
//!
//! Adding a new example is a pure data change: drop a folder with those
//! three files and the next `cargo test` run picks it up.

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn examples() {
    let examples_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut names: Vec<String> = fs::read_dir(&examples_root)
        .expect("read examples/")
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| {
            let dir = examples_root.join(name);
            dir.join(format!("{name}.fe")).exists() && dir.join("expected.txt").exists()
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no example fixtures found under examples/");

    let mut failures = Vec::new();
    for name in &names {
        if let Err(msg) = run_example(&examples_root, name) {
            failures.push(format!("• {name}: {msg}"));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} examples failed:\n{}",
        failures.len(),
        names.len(),
        failures.join("\n")
    );
}

fn run_example(root: &PathBuf, name: &str) -> Result<(), String> {
    let dir = root.join(name);
    let script = dir.join(format!("{name}.fe"));
    let expected = fs::read_to_string(dir.join("expected.txt"))
        .map_err(|e| format!("read expected.txt: {e}"))?;
    let expected_exit: i32 = fs::read_to_string(dir.join("exit"))
        .map_err(|e| format!("read exit: {e}"))?
        .trim()
        .parse()
        .map_err(|e| format!("exit must be an integer: {e}"))?;

    // Capture stdout + stderr into one file via a shared fd so interleaving
    // is preserved exactly as the shell's `2>&1` produces it.
    let combined_path = std::env::temp_dir().join(format!(
        "ferric-example-{name}-{pid}.out",
        pid = std::process::id()
    ));
    let f = File::create(&combined_path).map_err(|e| format!("tempfile: {e}"))?;
    let f_dup = f.try_clone().map_err(|e| format!("dup fd: {e}"))?;
    let status = Command::new(env!("CARGO_BIN_EXE_ferric"))
        .arg(&script)
        .stdout(Stdio::from(f))
        .stderr(Stdio::from(f_dup))
        .status()
        .map_err(|e| format!("spawn ferric: {e}"))?;
    let actual = fs::read_to_string(&combined_path)
        .map_err(|e| format!("read combined output: {e}"))?;
    let _ = fs::remove_file(&combined_path);

    let actual_exit = status.code().unwrap_or(-1);
    if actual_exit != expected_exit {
        return Err(format!(
            "exit {actual_exit} (expected {expected_exit})\noutput was:\n{actual}"
        ));
    }
    if actual != expected {
        return Err(format!(
            "output mismatch\n--- expected ---\n{expected}--- actual ---\n{actual}"
        ));
    }
    Ok(())
}
