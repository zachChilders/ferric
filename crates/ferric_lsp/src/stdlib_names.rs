//! Static catalog of stdlib function names and signatures.
//!
//! The stdlib is registered both in `register_stdlib` (runtime bodies) and in
//! `pipeline.rs::stdlib_native_fn_table` (resolver name + param info). Those
//! are the source of truth for which names exist; this catalog adds the
//! signature strings the LSP shows in completion `detail`. Keep in sync — a
//! drift here just means the completion menu shows a stale signature, not a
//! correctness bug.

pub const STDLIB_FUNCTIONS: &[(&str, &str)] = &[
    ("println",         "fn(s: Str) -> Unit"),
    ("print",           "fn(s: Str) -> Unit"),
    ("int_to_str",      "fn(n: Int) -> Str"),
    ("float_to_str",    "fn(n: Float) -> Str"),
    ("bool_to_str",     "fn(b: Bool) -> Str"),
    ("int_to_float",    "fn(n: Int) -> Float"),
    ("shell_stdout",    "fn(output: ShellOutput) -> Str"),
    ("shell_exit_code", "fn(output: ShellOutput) -> Int"),
    ("array_len",       "fn(arr: [T]) -> Int"),
    ("str_len",         "fn(s: Str) -> Int"),
    ("str_trim",        "fn(s: Str) -> Str"),
    ("str_contains",    "fn(s: Str, sub: Str) -> Bool"),
    ("str_starts_with", "fn(s: Str, prefix: Str) -> Bool"),
    ("str_parse_int",   "fn(s: Str) -> Option<Int>"),
    ("str_split",       "fn(s: Str, sep: Str) -> [Str]"),
    ("abs",             "fn(n: Int) -> Int"),
    ("min",             "fn(a: Int, b: Int) -> Int"),
    ("max",             "fn(a: Int, b: Int) -> Int"),
    ("sqrt",            "fn(n: Float) -> Float"),
    ("pow",             "fn(base: Float, exp: Float) -> Float"),
    ("floor",           "fn(n: Float) -> Int"),
    ("ceil",            "fn(n: Float) -> Int"),
    ("read_line",       "fn() -> Str"),
];
