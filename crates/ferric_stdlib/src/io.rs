//! # `io` — File I/O and Standard Streams
//!
//! Implements the `io` module from `docs/stdlib.md`. Functions here are
//! registered under the `io_*` prefix and live alongside the legacy
//! `print`/`println`/`read_line` natives in `lib.rs`.
//!
//! All file APIs return `Result<T, IoError>` where errors are reported as
//! native error strings keyed on `std::io::ErrorKind`.

// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                                  // shared with str/bytes task
//   Time(...)                                       // shared with time task
//   FileWriter(Rc<RefCell<BufWriter<File>>>)        // see file_writer / write / writeln / flush / close

// REQUIRED DEPS:
//   none — std::fs, std::io, std::path are sufficient. The `Time` value
//   produced by io_modified_at wraps std::time::SystemTime; the time task
//   chooses the canonical `Time` representation during integration.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::rc::Rc;

use ferric_common::Interner;

use crate::{NativeRegistry, NativeValue};

// ============================================================================
// Local helpers (private to this module per stdlib-00 rule 5)
// ============================================================================

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!("expected {} argument(s), got {}", expected, args.len()))
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

#[allow(dead_code)]
fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {:?}", other)),
    }
}

/// Map an `io::Error` into the documented `IoError::*` native error string.
fn io_err(path: &str, e: io::Error) -> String {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => format!("IoError::NotFound {{ path: {:?} }}", path),
        PermissionDenied => format!("IoError::PermissionDenied {{ path: {:?} }}", path),
        AlreadyExists => format!("IoError::AlreadyExists {{ path: {:?} }}", path),
        Interrupted => "IoError::Interrupted {}".to_string(),
        // Two best-effort kinds — std::io currently doesn't expose stable
        // IsADirectory/NotADirectory on stable, so map via raw OS error
        // where possible and fall back to Other.
        _ => {
            #[cfg(unix)]
            {
                if let Some(code) = e.raw_os_error() {
                    // EISDIR == 21, ENOTDIR == 20, ENOTEMPTY == 66 (mac) / 39 (linux)
                    if code == 21 {
                        return format!("IoError::IsDirectory {{ path: {:?} }}", path);
                    }
                    if code == 20 {
                        return format!("IoError::IsFile {{ path: {:?} }}", path);
                    }
                    if code == 66 || code == 39 {
                        return format!("IoError::NotEmpty {{ path: {:?} }}", path);
                    }
                }
            }
            format!("IoError::Other {{ message: {:?} }}", e.to_string())
        }
    }
}

// ============================================================================
// Standard streams (mirror existing print/println/read_line under io_* names)
// ============================================================================

fn builtin_io_print(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    print!("{}", s);
    Ok(NativeValue::Unit)
}

fn builtin_io_println(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    println!("{}", s);
    Ok(NativeValue::Unit)
}

fn builtin_io_eprint(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    eprint!("{}", s);
    Ok(NativeValue::Unit)
}

fn builtin_io_eprintln(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    eprintln!("{}", s);
    Ok(NativeValue::Unit)
}

fn builtin_io_read_line(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| io_err("<stdin>", e))?;
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(NativeValue::Str(line))
}

fn builtin_io_read_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| io_err("<stdin>", e))?;
    Ok(NativeValue::Str(buf))
}

// ============================================================================
// File — text
// ============================================================================

fn builtin_io_read_file(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    fs::read_to_string(path.as_str())
        .map(NativeValue::Str)
        .map_err(|e| io_err(path, e))
}

fn builtin_io_write_file(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let path = expect_str(&args[0])?;
    let content = expect_str(&args[1])?;
    fs::write(path.as_str(), content.as_bytes())
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(path, e))
}

fn builtin_io_append_file(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let path = expect_str(&args[0])?;
    let content = expect_str(&args[1])?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_str())
        .map_err(|e| io_err(path, e))?;
    f.write_all(content.as_bytes()).map_err(|e| io_err(path, e))?;
    Ok(NativeValue::Unit)
}

fn builtin_io_read_lines(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    let contents = fs::read_to_string(path.as_str()).map_err(|e| io_err(path, e))?;
    // Split on '\n' and strip a trailing '\r' from each line.
    let mut lines: Vec<NativeValue> = contents
        .split('\n')
        .map(|line| {
            let trimmed = if let Some(stripped) = line.strip_suffix('\r') { stripped } else { line };
            NativeValue::Str(trimmed.to_string())
        })
        .collect();
    // If the file ended with a newline, the split produced a trailing
    // empty element — drop it for ergonomic line iteration.
    if let Some(NativeValue::Str(last)) = lines.last() {
        if last.is_empty() && contents.ends_with('\n') {
            lines.pop();
        }
    }
    Ok(NativeValue::Array(lines))
}

// ============================================================================
// File — bytes (Bytes variant declared as required NativeValue extension)
// ============================================================================
//
// During the parallel phase NativeValue::Bytes does not yet exist. We emulate
// it with NativeValue::Array(NativeValue::Int 0..=255) so the tests can run
// today; integration will swap these to the real Bytes variant.

fn bytes_to_array(bytes: &[u8]) -> NativeValue {
    NativeValue::Array(bytes.iter().map(|b| NativeValue::Int(*b as i64)).collect())
}

fn array_to_bytes(value: &NativeValue) -> Result<Vec<u8>, String> {
    match value {
        NativeValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    NativeValue::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
                    NativeValue::Int(n) => {
                        return Err(format!("byte out of range 0..=255: {}", n));
                    }
                    other => return Err(format!("expected byte (Int), got {:?}", other)),
                }
            }
            Ok(out)
        }
        other => Err(format!("expected bytes, got {:?}", other)),
    }
}

fn builtin_io_read_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    let bytes = fs::read(path.as_str()).map_err(|e| io_err(path, e))?;
    Ok(bytes_to_array(&bytes))
}

fn builtin_io_write_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let path = expect_str(&args[0])?;
    let bytes = array_to_bytes(&args[1])?;
    fs::write(path.as_str(), &bytes)
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(path, e))
}

// ============================================================================
// File metadata
// ============================================================================

fn builtin_io_exists(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    Ok(NativeValue::Bool(Path::new(path.as_str()).exists()))
}

fn builtin_io_is_file(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    Ok(NativeValue::Bool(Path::new(path.as_str()).is_file()))
}

fn builtin_io_is_dir(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    Ok(NativeValue::Bool(Path::new(path.as_str()).is_dir()))
}

fn builtin_io_file_size(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    let meta = fs::metadata(path.as_str()).map_err(|e| io_err(path, e))?;
    Ok(NativeValue::Int(meta.len() as i64))
}

fn builtin_io_modified_at(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    let meta = fs::metadata(path.as_str()).map_err(|e| io_err(path, e))?;
    let modified = meta.modified().map_err(|e| io_err(path, e))?;
    // The `time` task picks the final Time representation; we surface
    // milliseconds since the unix epoch as an Int placeholder. Integration
    // will rewrap this in the real Time variant.
    let dur = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("IoError::Other {{ message: {:?} }}", e.to_string()))?;
    Ok(NativeValue::Int(dur.as_millis() as i64))
}

// ============================================================================
// Directory
// ============================================================================

fn builtin_io_list_dir(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    let mut out: Vec<NativeValue> = Vec::new();
    let entries = fs::read_dir(path.as_str()).map_err(|e| io_err(path, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_err(path, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        out.push(NativeValue::Str(name));
    }
    out.sort_by(|a, b| match (a, b) {
        (NativeValue::Str(x), NativeValue::Str(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    });
    Ok(NativeValue::Array(out))
}

fn list_dir_recursive_inner(
    base: &Path,
    current: &Path,
    out: &mut Vec<NativeValue>,
) -> Result<(), io::Error> {
    let entries = fs::read_dir(current)?;
    let mut sorted: Vec<_> = entries.collect::<Result<Vec<_>, _>>()?;
    sorted.sort_by_key(|e| e.file_name());
    for entry in sorted {
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap_or(&path);
        out.push(NativeValue::Str(rel.to_string_lossy().into_owned()));
        if path.is_dir() {
            list_dir_recursive_inner(base, &path, out)?;
        }
    }
    Ok(())
}

fn builtin_io_list_dir_recursive(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    let base = PathBuf::from(path.as_str());
    let mut out = Vec::new();
    list_dir_recursive_inner(&base, &base, &mut out).map_err(|e| io_err(path, e))?;
    Ok(NativeValue::Array(out))
}

fn builtin_io_make_dir(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    fs::create_dir(path.as_str())
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(path, e))
}

fn builtin_io_make_dir_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    fs::create_dir_all(path.as_str())
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(path, e))
}

fn builtin_io_remove_file(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    fs::remove_file(path.as_str())
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(path, e))
}

fn builtin_io_remove_dir(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    fs::remove_dir(path.as_str())
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(path, e))
}

fn builtin_io_remove_dir_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    fs::remove_dir_all(path.as_str())
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(path, e))
}

fn builtin_io_rename(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let from = expect_str(&args[0])?;
    let to = expect_str(&args[1])?;
    fs::rename(from.as_str(), to.as_str())
        .map(|()| NativeValue::Unit)
        .map_err(|e| io_err(from, e))
}

fn builtin_io_copy(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let from = expect_str(&args[0])?;
    let to = expect_str(&args[1])?;
    fs::copy(from.as_str(), to.as_str())
        .map(|n| NativeValue::Int(n as i64))
        .map_err(|e| io_err(from, e))
}

// ============================================================================
// Buffered FileWriter
// ============================================================================
//
// `FileWriter` is declared as a required NativeValue variant. Until
// integration adds it, we model the handle by interning a stable Str id and
// keeping the BufWriter in a thread-local table. This keeps the *behaviour*
// of write/writeln/flush/close exercised end-to-end so integration only
// needs to swap the handle representation, not the semantics.

thread_local! {
    static FILE_WRITERS: RefCell<Vec<Option<BufWriter<File>>>> = RefCell::new(Vec::new());
    static FILE_WRITER_PATHS: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

const FILE_WRITER_TAG: &str = "__ferric_filewriter__:";

fn fw_alloc(path: String, writer: BufWriter<File>) -> usize {
    FILE_WRITERS.with(|cell| {
        let mut slots = cell.borrow_mut();
        slots.push(Some(writer));
        slots.len() - 1
    });
    FILE_WRITER_PATHS.with(|cell| {
        let mut paths = cell.borrow_mut();
        paths.push(path);
    });
    FILE_WRITERS.with(|cell| cell.borrow().len() - 1)
}

fn fw_handle_to_str(idx: usize) -> String {
    format!("{}{}", FILE_WRITER_TAG, idx)
}

fn fw_str_to_idx(s: &str) -> Result<usize, String> {
    let rest = s
        .strip_prefix(FILE_WRITER_TAG)
        .ok_or_else(|| format!("expected FileWriter handle, got {:?}", s))?;
    rest.parse::<usize>()
        .map_err(|_| format!("invalid FileWriter handle: {:?}", s))
}

fn fw_path(idx: usize) -> String {
    FILE_WRITER_PATHS.with(|cell| {
        cell.borrow()
            .get(idx)
            .cloned()
            .unwrap_or_else(|| "<closed>".to_string())
    })
}

fn fw_with<F>(idx: usize, f: F) -> Result<NativeValue, String>
where
    F: FnOnce(&mut BufWriter<File>) -> Result<NativeValue, io::Error>,
{
    let path = fw_path(idx);
    FILE_WRITERS.with(|cell| {
        let mut slots = cell.borrow_mut();
        match slots.get_mut(idx).and_then(|slot| slot.as_mut()) {
            Some(w) => f(w).map_err(|e| io_err(&path, e)),
            None => Err(format!("IoError::Other {{ message: \"FileWriter is closed\" }}")),
        }
    })
}

fn fw_take(idx: usize) -> Option<BufWriter<File>> {
    FILE_WRITERS.with(|cell| {
        let mut slots = cell.borrow_mut();
        slots.get_mut(idx).and_then(|slot| slot.take())
    })
}

fn builtin_io_file_writer(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let path = expect_str(&args[0])?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path.as_str())
        .map_err(|e| io_err(path, e))?;
    let writer = BufWriter::new(file);
    let idx = fw_alloc(path.clone(), writer);
    Ok(NativeValue::Str(fw_handle_to_str(idx)))
}

fn builtin_io_write(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let handle = expect_str(&args[0])?;
    let s = expect_str(&args[1])?;
    let idx = fw_str_to_idx(handle)?;
    fw_with(idx, |w| {
        w.write_all(s.as_bytes())?;
        Ok(NativeValue::Unit)
    })
}

fn builtin_io_writeln(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let handle = expect_str(&args[0])?;
    let s = expect_str(&args[1])?;
    let idx = fw_str_to_idx(handle)?;
    fw_with(idx, |w| {
        w.write_all(s.as_bytes())?;
        w.write_all(b"\n")?;
        Ok(NativeValue::Unit)
    })
}

fn builtin_io_flush(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let handle = expect_str(&args[0])?;
    let idx = fw_str_to_idx(handle)?;
    fw_with(idx, |w| {
        w.flush()?;
        Ok(NativeValue::Unit)
    })
}

fn builtin_io_close(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let handle = expect_str(&args[0])?;
    let idx = fw_str_to_idx(handle)?;
    let path = fw_path(idx);
    let mut writer = fw_take(idx)
        .ok_or_else(|| format!("IoError::Other {{ message: \"FileWriter already closed\" }}"))?;
    writer.flush().map_err(|e| io_err(&path, e))?;
    drop(writer); // closes the file
    Ok(NativeValue::Unit)
}

// Suppress the "unused" lint on the Rc/RefCell imports until integration
// switches FileWriter to the real Rc<RefCell<BufWriter<File>>> variant.
#[allow(dead_code)]
type _RcRefCellPlaceholder = Rc<RefCell<BufWriter<File>>>;

// ============================================================================
// Registration
// ============================================================================

/// Registers all `io_*` native functions with the registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    // Standard streams
    registry.register(interner.intern("io_print"), builtin_io_print);
    registry.register(interner.intern("io_println"), builtin_io_println);
    registry.register(interner.intern("io_eprint"), builtin_io_eprint);
    registry.register(interner.intern("io_eprintln"), builtin_io_eprintln);
    registry.register(interner.intern("io_read_line"), builtin_io_read_line);
    registry.register(interner.intern("io_read_all"), builtin_io_read_all);

    // File — text
    registry.register(interner.intern("io_read_file"), builtin_io_read_file);
    registry.register(interner.intern("io_write_file"), builtin_io_write_file);
    registry.register(interner.intern("io_append_file"), builtin_io_append_file);
    registry.register(interner.intern("io_read_lines"), builtin_io_read_lines);

    // File — bytes
    registry.register(interner.intern("io_read_bytes"), builtin_io_read_bytes);
    registry.register(interner.intern("io_write_bytes"), builtin_io_write_bytes);

    // Metadata
    registry.register(interner.intern("io_exists"), builtin_io_exists);
    registry.register(interner.intern("io_is_file"), builtin_io_is_file);
    registry.register(interner.intern("io_is_dir"), builtin_io_is_dir);
    registry.register(interner.intern("io_file_size"), builtin_io_file_size);
    registry.register(interner.intern("io_modified_at"), builtin_io_modified_at);

    // Directory
    registry.register(interner.intern("io_list_dir"), builtin_io_list_dir);
    registry.register(
        interner.intern("io_list_dir_recursive"),
        builtin_io_list_dir_recursive,
    );
    registry.register(interner.intern("io_make_dir"), builtin_io_make_dir);
    registry.register(interner.intern("io_make_dir_all"), builtin_io_make_dir_all);
    registry.register(interner.intern("io_remove_file"), builtin_io_remove_file);
    registry.register(interner.intern("io_remove_dir"), builtin_io_remove_dir);
    registry.register(
        interner.intern("io_remove_dir_all"),
        builtin_io_remove_dir_all,
    );
    registry.register(interner.intern("io_rename"), builtin_io_rename);
    registry.register(interner.intern("io_copy"), builtin_io_copy);

    // FileWriter
    registry.register(interner.intern("io_file_writer"), builtin_io_file_writer);
    registry.register(interner.intern("io_write"), builtin_io_write);
    registry.register(interner.intern("io_writeln"), builtin_io_writeln);
    registry.register(interner.intern("io_flush"), builtin_io_flush);
    registry.register(interner.intern("io_close"), builtin_io_close);
}

// Silence dead_code on BufRead: not used directly but kept for any future
// streaming reader; also avoid an unused-import warning under all platforms.
#[allow(dead_code)]
fn _silence_bufread<R: BufRead>(_r: R) {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// RAII guard: deletes the path tree on drop, ignoring errors.
    struct TempPath(PathBuf);

    impl TempPath {
        fn as_str(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            if self.0.is_dir() {
                let _ = fs::remove_dir_all(&self.0);
            } else if self.0.exists() {
                let _ = fs::remove_file(&self.0);
            }
        }
    }

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_temp(label: &str) -> TempPath {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "ferric_stdlib_test_{}_{}_{}_{}",
            module_path!(),
            std::process::id(),
            n,
            label,
        ));
        TempPath(path)
    }

    fn unique_temp_dir(label: &str) -> TempPath {
        let tp = unique_temp(label);
        fs::create_dir_all(&tp.0).expect("create unique temp dir");
        tp
    }

    fn s(v: &str) -> NativeValue {
        NativeValue::Str(v.to_string())
    }

    // ---- Standard streams ------------------------------------------------

    #[test]
    fn io_print_returns_unit() {
        let r = builtin_io_print(&[s("")]).unwrap();
        assert_eq!(r, NativeValue::Unit);
    }

    #[test]
    fn io_println_returns_unit() {
        let r = builtin_io_println(&[s("")]).unwrap();
        assert_eq!(r, NativeValue::Unit);
    }

    #[test]
    fn io_eprint_and_eprintln_return_unit() {
        assert_eq!(builtin_io_eprint(&[s("")]).unwrap(), NativeValue::Unit);
        assert_eq!(builtin_io_eprintln(&[s("")]).unwrap(), NativeValue::Unit);
    }

    // ---- read_file / write_file / append_file / read_lines ---------------

    #[test]
    fn write_and_read_file_roundtrip() {
        let tp = unique_temp("rw");
        let p = tp.as_str();
        builtin_io_write_file(&[s(&p), s("hello world")]).unwrap();
        let r = builtin_io_read_file(&[s(&p)]).unwrap();
        assert_eq!(r, NativeValue::Str("hello world".to_string()));
    }

    #[test]
    fn append_file_extends() {
        let tp = unique_temp("append");
        let p = tp.as_str();
        builtin_io_write_file(&[s(&p), s("a\n")]).unwrap();
        builtin_io_append_file(&[s(&p), s("b\n")]).unwrap();
        builtin_io_append_file(&[s(&p), s("c\n")]).unwrap();
        let r = builtin_io_read_file(&[s(&p)]).unwrap();
        assert_eq!(r, NativeValue::Str("a\nb\nc\n".to_string()));
    }

    #[test]
    fn read_lines_returns_n_lines() {
        let tp = unique_temp("lines");
        let p = tp.as_str();
        builtin_io_write_file(&[s(&p), s("one\ntwo\r\nthree")]).unwrap();
        let r = builtin_io_read_lines(&[s(&p)]).unwrap();
        match r {
            NativeValue::Array(items) => {
                let strs: Vec<String> = items
                    .into_iter()
                    .map(|v| match v {
                        NativeValue::Str(s) => s,
                        _ => panic!("non-string line"),
                    })
                    .collect();
                assert_eq!(strs, vec!["one", "two", "three"]);
            }
            _ => panic!("expected Array"),
        }
    }

    #[test]
    fn read_lines_strips_trailing_empty_when_file_ends_in_newline() {
        let tp = unique_temp("lines2");
        let p = tp.as_str();
        builtin_io_write_file(&[s(&p), s("a\nb\n")]).unwrap();
        let r = builtin_io_read_lines(&[s(&p)]).unwrap();
        if let NativeValue::Array(items) = r {
            assert_eq!(items.len(), 2);
        } else {
            panic!("expected Array")
        }
    }

    // ---- bytes ----------------------------------------------------------

    #[test]
    fn write_bytes_then_read_bytes_roundtrip() {
        let tp = unique_temp("bytes");
        let p = tp.as_str();
        let bytes = NativeValue::Array(vec![
            NativeValue::Int(0),
            NativeValue::Int(1),
            NativeValue::Int(255),
            NativeValue::Int(128),
        ]);
        builtin_io_write_bytes(&[s(&p), bytes.clone()]).unwrap();
        let r = builtin_io_read_bytes(&[s(&p)]).unwrap();
        assert_eq!(r, bytes);
    }

    #[test]
    fn write_bytes_rejects_out_of_range() {
        let tp = unique_temp("bytes_bad");
        let p = tp.as_str();
        let bad = NativeValue::Array(vec![NativeValue::Int(256)]);
        let err = builtin_io_write_bytes(&[s(&p), bad]).unwrap_err();
        assert!(err.contains("byte out of range"));
    }

    // ---- exists / is_file / is_dir / file_size --------------------------

    #[test]
    fn exists_is_file_is_dir_file_size() {
        let dir = unique_temp_dir("metadata");
        let dpath = dir.as_str();
        let mut fpath = PathBuf::from(&dpath);
        fpath.push("f.txt");
        let fpath_s = fpath.to_string_lossy().into_owned();

        builtin_io_write_file(&[s(&fpath_s), s("12345")]).unwrap();

        assert_eq!(builtin_io_exists(&[s(&fpath_s)]).unwrap(), NativeValue::Bool(true));
        assert_eq!(builtin_io_is_file(&[s(&fpath_s)]).unwrap(), NativeValue::Bool(true));
        assert_eq!(builtin_io_is_file(&[s(&dpath)]).unwrap(), NativeValue::Bool(false));
        assert_eq!(builtin_io_is_dir(&[s(&dpath)]).unwrap(), NativeValue::Bool(true));
        assert_eq!(builtin_io_is_dir(&[s(&fpath_s)]).unwrap(), NativeValue::Bool(false));
        assert_eq!(builtin_io_file_size(&[s(&fpath_s)]).unwrap(), NativeValue::Int(5));
    }

    #[test]
    fn exists_returns_false_for_missing() {
        let tp = unique_temp("missing");
        let p = tp.as_str();
        assert_eq!(builtin_io_exists(&[s(&p)]).unwrap(), NativeValue::Bool(false));
    }

    #[test]
    fn modified_at_returns_int_ms() {
        let tp = unique_temp("modified");
        let p = tp.as_str();
        builtin_io_write_file(&[s(&p), s("x")]).unwrap();
        let r = builtin_io_modified_at(&[s(&p)]).unwrap();
        match r {
            NativeValue::Int(n) => assert!(n > 0),
            _ => panic!("expected Int"),
        }
    }

    // ---- Directory ops --------------------------------------------------

    #[test]
    fn make_dir_and_remove_dir() {
        let tp = unique_temp("mkdir");
        let p = tp.as_str();
        builtin_io_make_dir(&[s(&p)]).unwrap();
        assert_eq!(builtin_io_is_dir(&[s(&p)]).unwrap(), NativeValue::Bool(true));
        builtin_io_remove_dir(&[s(&p)]).unwrap();
        assert_eq!(builtin_io_exists(&[s(&p)]).unwrap(), NativeValue::Bool(false));
    }

    #[test]
    fn make_dir_all_creates_intermediate_components() {
        let tp = unique_temp("mkdirall");
        let mut deep = PathBuf::from(&tp.0);
        deep.push("a/b/c");
        let deep_s = deep.to_string_lossy().into_owned();
        builtin_io_make_dir_all(&[s(&deep_s)]).unwrap();
        assert_eq!(builtin_io_is_dir(&[s(&deep_s)]).unwrap(), NativeValue::Bool(true));
    }

    #[test]
    fn remove_file_removes() {
        let tp = unique_temp("rmfile");
        let p = tp.as_str();
        builtin_io_write_file(&[s(&p), s("x")]).unwrap();
        builtin_io_remove_file(&[s(&p)]).unwrap();
        assert_eq!(builtin_io_exists(&[s(&p)]).unwrap(), NativeValue::Bool(false));
    }

    #[test]
    fn remove_dir_all_removes_recursively() {
        let tp = unique_temp("rmrf");
        let mut deep = PathBuf::from(&tp.0);
        deep.push("a/b/c");
        let deep_s = deep.to_string_lossy().into_owned();
        builtin_io_make_dir_all(&[s(&deep_s)]).unwrap();
        let mut file = PathBuf::from(&deep);
        file.push("f.txt");
        builtin_io_write_file(&[s(&file.to_string_lossy()), s("hi")]).unwrap();
        builtin_io_remove_dir_all(&[s(&tp.as_str())]).unwrap();
        assert_eq!(
            builtin_io_exists(&[s(&tp.as_str())]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn rename_moves_a_file() {
        let dir = unique_temp_dir("rename");
        let mut from = PathBuf::from(&dir.0);
        from.push("from.txt");
        let mut to = PathBuf::from(&dir.0);
        to.push("to.txt");
        let from_s = from.to_string_lossy().into_owned();
        let to_s = to.to_string_lossy().into_owned();
        builtin_io_write_file(&[s(&from_s), s("payload")]).unwrap();
        builtin_io_rename(&[s(&from_s), s(&to_s)]).unwrap();
        assert_eq!(builtin_io_exists(&[s(&from_s)]).unwrap(), NativeValue::Bool(false));
        assert_eq!(builtin_io_read_file(&[s(&to_s)]).unwrap(), NativeValue::Str("payload".into()));
    }

    #[test]
    fn copy_returns_byte_count() {
        let dir = unique_temp_dir("copy");
        let mut from = PathBuf::from(&dir.0);
        from.push("from.txt");
        let mut to = PathBuf::from(&dir.0);
        to.push("to.txt");
        let from_s = from.to_string_lossy().into_owned();
        let to_s = to.to_string_lossy().into_owned();
        builtin_io_write_file(&[s(&from_s), s("12345")]).unwrap();
        let r = builtin_io_copy(&[s(&from_s), s(&to_s)]).unwrap();
        assert_eq!(r, NativeValue::Int(5));
        assert_eq!(builtin_io_read_file(&[s(&to_s)]).unwrap(), NativeValue::Str("12345".into()));
    }

    // ---- Errors ---------------------------------------------------------

    #[test]
    fn read_file_missing_path_yields_not_found() {
        let tp = unique_temp("missing_read");
        let p = tp.as_str();
        let err = builtin_io_read_file(&[s(&p)]).unwrap_err();
        assert!(err.starts_with("IoError::NotFound"), "got: {}", err);
    }

    #[test]
    fn make_dir_already_exists() {
        let dir = unique_temp_dir("dup");
        let p = dir.as_str();
        let err = builtin_io_make_dir(&[s(&p)]).unwrap_err();
        assert!(err.starts_with("IoError::AlreadyExists"), "got: {}", err);
    }

    // ---- list_dir / list_dir_recursive ----------------------------------

    #[test]
    fn list_dir_returns_short_names() {
        let dir = unique_temp_dir("ls");
        let mut a = PathBuf::from(&dir.0);
        a.push("a.txt");
        let mut b = PathBuf::from(&dir.0);
        b.push("b.txt");
        builtin_io_write_file(&[s(&a.to_string_lossy()), s("a")]).unwrap();
        builtin_io_write_file(&[s(&b.to_string_lossy()), s("b")]).unwrap();
        let r = builtin_io_list_dir(&[s(&dir.as_str())]).unwrap();
        if let NativeValue::Array(items) = r {
            let names: Vec<String> = items
                .into_iter()
                .map(|v| match v {
                    NativeValue::Str(s) => s,
                    _ => panic!(),
                })
                .collect();
            assert_eq!(names, vec!["a.txt", "b.txt"]);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn list_dir_recursive_walks_subdirs() {
        let dir = unique_temp_dir("walk");
        let mut sub = PathBuf::from(&dir.0);
        sub.push("sub");
        fs::create_dir(&sub).unwrap();
        let mut f1 = PathBuf::from(&dir.0);
        f1.push("top.txt");
        let mut f2 = PathBuf::from(&sub);
        f2.push("inner.txt");
        builtin_io_write_file(&[s(&f1.to_string_lossy()), s("a")]).unwrap();
        builtin_io_write_file(&[s(&f2.to_string_lossy()), s("b")]).unwrap();

        let r = builtin_io_list_dir_recursive(&[s(&dir.as_str())]).unwrap();
        if let NativeValue::Array(items) = r {
            let names: Vec<String> = items
                .into_iter()
                .map(|v| match v {
                    NativeValue::Str(s) => s,
                    _ => panic!(),
                })
                .collect();
            // Paths are relative to the input directory and OS-native slashes.
            let sub_str = "sub";
            let inner_str = format!("sub{}inner.txt", std::path::MAIN_SEPARATOR);
            assert!(names.contains(&sub_str.to_string()), "{:?}", names);
            assert!(names.contains(&inner_str), "{:?}", names);
            assert!(names.contains(&"top.txt".to_string()), "{:?}", names);
        } else {
            panic!("expected Array");
        }
    }

    // ---- FileWriter -----------------------------------------------------

    #[test]
    fn file_writer_write_writeln_close_produces_expected_file() {
        let tp = unique_temp("fw");
        let p = tp.as_str();
        let handle = builtin_io_file_writer(&[s(&p)]).unwrap();
        let h = match &handle {
            NativeValue::Str(_) => handle.clone(),
            _ => panic!("expected Str handle"),
        };
        builtin_io_write(&[h.clone(), s("hello ")]).unwrap();
        builtin_io_writeln(&[h.clone(), s("world")]).unwrap();
        builtin_io_write(&[h.clone(), s("done")]).unwrap();
        builtin_io_flush(&[h.clone()]).unwrap();
        builtin_io_close(&[h.clone()]).unwrap();

        let r = builtin_io_read_file(&[s(&p)]).unwrap();
        assert_eq!(r, NativeValue::Str("hello world\ndone".to_string()));
    }

    #[test]
    fn file_writer_close_then_write_errs() {
        let tp = unique_temp("fwclose");
        let p = tp.as_str();
        let handle = builtin_io_file_writer(&[s(&p)]).unwrap();
        builtin_io_close(&[handle.clone()]).unwrap();
        let err = builtin_io_write(&[handle, s("x")]).unwrap_err();
        assert!(err.contains("closed"), "got: {}", err);
    }

    #[test]
    fn register_installs_all_io_natives() {
        let mut reg = NativeRegistry::new();
        let mut int = Interner::new();
        register(&mut reg, &mut int);

        for name in [
            "io_print",
            "io_println",
            "io_eprint",
            "io_eprintln",
            "io_read_line",
            "io_read_all",
            "io_read_file",
            "io_write_file",
            "io_append_file",
            "io_read_lines",
            "io_read_bytes",
            "io_write_bytes",
            "io_exists",
            "io_is_file",
            "io_is_dir",
            "io_file_size",
            "io_modified_at",
            "io_list_dir",
            "io_list_dir_recursive",
            "io_make_dir",
            "io_make_dir_all",
            "io_remove_file",
            "io_remove_dir",
            "io_remove_dir_all",
            "io_rename",
            "io_copy",
            "io_file_writer",
            "io_write",
            "io_writeln",
            "io_flush",
            "io_close",
        ] {
            let sym = int.intern(name);
            assert!(reg.get(sym).is_some(), "missing native: {}", name);
        }
    }
}
