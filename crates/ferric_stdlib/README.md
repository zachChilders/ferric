# ferric_stdlib

Runtime implementation of Ferric's standard library. Provides ~100 native functions callable from Ferric programs.

## Registration

```rust
pub fn register_stdlib(natives: &mut NativeRegistry, interner: &mut Interner)
```

This is the only public entry point. It calls each module's `register()` function in turn, binding native function names to Rust closures in the `NativeRegistry`.

## Modules

| Module | Functions |
|---|---|
| `str_` | length, slice, split, trim, starts_with, ends_with, contains, replace, to_upper, to_lower, … |
| `bytes` | byte-level array operations |
| `list` | map, filter, fold, zip, flatten, enumerate, sort_by, … |
| `sort` | stable sort with a Ferric closure comparator |
| `map` | insert, get, remove, keys, values, contains_key, … |
| `set` | insert, contains, union, intersection, … |
| `math` | abs, floor, ceil, sqrt, pow, log, sin, cos, min, max, … |
| `io` | read_file, write_file, append_file, file_exists, … |
| `path` | join, dirname, basename, extension, … |
| `env` | get_env, set_env, env_vars |
| `time_` | now, sleep, duration arithmetic |
| `rand_` | random int/float, shuffle |
| `re` | match, find_all, split, replace (regex) |
| `json` | parse, stringify |
| `fmt` | format strings |
| `encode` | base64 encode/decode, URL encode/decode |
| `crypto` | sha256, blake3, hmac, ed25519 sign/verify |
| `http` | get, post, put, delete (blocking HTTP client) |
| `serve` | start a minimal HTTP server |
| `sock` | raw TCP socket connect/read/write |
| `proc_` | spawn subprocesses, capture output |
| `log` | structured logging |
| `sync` | mutex, channel, once |
| `dotenv` | load `.env` files into the environment |
| `llm` | LLM API integration |

## Native value types

Native functions receive and return `NativeValue`, a flattened mirror of `ferric_vm::Value`. The VM translates between them at the call boundary, keeping `ferric_stdlib` independent of `ferric_vm` internals.

Higher-order functions (e.g. `map`, `filter`, `sort_by`) accept `NativeClosure` — a wrapper that lets Ferric closures be invoked from Rust.

## Type metadata

Parameter names and type signatures are defined separately in `ferric_stdlib_meta` (no heavy dependencies) so the resolver and type checker can use them without linking stdlib's full dependency tree.
