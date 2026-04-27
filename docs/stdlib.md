# Ferric Standard Library — Design Specification

> The stdlib is the language's personality. Ferric's stdlib is **small by count,
> large by power**. Every module is cohesive, every name is deliberate, and nothing
> is in here just because another language has it. If a function exists, it has a
> point of view.

---

## Design Principles

**1. Results everywhere, panics nowhere.**
No stdlib function panics on bad input. Every failure returns `Result<T, E>` or
`Option<T>`. The caller decides what "bad input" means.

**2. Named parameters are the calling convention.**
All stdlib functions follow the same named-parameter convention as user functions.
`str::split(s: haystack, on: ",")` reads like a sentence.

**3. Modules are namespaces, not classes.**
`http::get(url: u)` not `u.get()`. Methods on values come from traits (M5+).
Until traits arrive, stdlib functions are free functions in named modules.

**4. Bytes are not strings. Strings are not bytes.**
`Str` is always valid UTF-8. `Bytes` is always raw octets. Conversion is explicit
and goes through `Result`. No silent coercions, no replacement characters unless
you asked for them.

**5. The iron standard.**
Names should be crisp and unambiguous. No `doTheThing`, no `getX`/`setX`,
no Hungarian notation. If a function name needs the word "get", something
went wrong upstream.

---

## Module Index

| Module | What it covers |
|---|---|
| `str` | UTF-8 string manipulation |
| `bytes` | Raw byte sequences |
| `list` | Dynamic array operations |
| `map` | Hash map operations |
| `set` | Hash set operations |
| `sort` | Sorting and ordering |
| `math` | Numeric operations |
| `io` | File I/O, stdin/stdout |
| `path` | Filesystem path manipulation |
| `env` | Environment variables, process args |
| `http` | HTTP client (fetch-like) |
| `serve` | HTTP server (Fastify-like) |
| `sock` | TCP/UDP sockets |
| `crypto` | Hashing, HMAC, encryption |
| `encode` | Base64, hex, URL encoding |
| `json` | JSON serialisation/deserialisation |
| `time` | Timestamps, durations, formatting |
| `rand` | Random number generation |
| `re` | Regular expressions |
| `fmt` | String formatting and interpolation |
| `proc` | Subprocess management (beyond `$`) |
| `sync` | Channels, mutexes (async milestone) |
| `log` | Structured logging |

---

## `str` — String Manipulation

All functions operate on `Str` (guaranteed UTF-8). Character-level operations
count Unicode scalar values, not bytes. Byte-level operations live in `bytes`.

```rust
// Length — characters, not bytes
str::len(s: Str) -> Int

// Slicing — character indices; returns Result to avoid silent truncation
str::slice(s: Str, from: Int, to: Int) -> Result<Str, StrError>

// Search
str::contains(s: Str, needle: Str) -> Bool
str::starts_with(s: Str, prefix: Str) -> Bool
str::ends_with(s: Str, suffix: Str) -> Bool
str::find(s: Str, needle: Str) -> Option<Int>          // index of first match
str::find_all(s: Str, needle: Str) -> List<Int>        // indices of all matches

// Splitting
str::split(s: Str, on: Str) -> List<Str>
str::split_once(s: Str, on: Str) -> Option<(Str, Str)>  // left and right of first match
str::lines(s: Str) -> List<Str>                          // splits on \n and \r\n
str::chars(s: Str) -> List<Str>                          // one string per codepoint

// Trimming
str::trim(s: Str) -> Str
str::trim_start(s: Str) -> Str
str::trim_end(s: Str) -> Str
str::trim_chars(s: Str, chars: Str) -> Str               // trim any char in `chars`

// Case
str::to_upper(s: Str) -> Str
str::to_lower(s: Str) -> Str
str::to_title(s: Str) -> Str                             // Title Case each word

// Padding and alignment
str::pad_start(s: Str, to: Int, with: Str = " ") -> Str
str::pad_end(s: Str, to: Int, with: Str = " ") -> Str
str::center(s: Str, to: Int, with: Str = " ") -> Str

// Replace
str::replace(s: Str, from: Str, to: Str) -> Str          // replaces all occurrences
str::replace_first(s: Str, from: Str, to: Str) -> Str
str::replace_n(s: Str, from: Str, to: Str, n: Int) -> Str

// Join
str::join(parts: List<Str>, sep: Str) -> Str

// Repeat
str::repeat(s: Str, n: Int) -> Str

// Parse
str::parse_int(s: Str) -> Result<Int, StrError>
str::parse_float(s: Str) -> Result<Float, StrError>
str::parse_bool(s: Str) -> Result<Bool, StrError>         // "true"/"false" only

// Comparison
str::eq_ignore_case(a: Str, b: Str) -> Bool

// Bytes interop
str::to_bytes(s: Str) -> Bytes                            // UTF-8 encoded
str::from_bytes(b: Bytes) -> Result<Str, StrError>        // fails on invalid UTF-8
str::from_bytes_lossy(b: Bytes) -> Str                    // replaces invalid sequences with U+FFFD

// Check
str::is_empty(s: Str) -> Bool
str::is_ascii(s: Str) -> Bool
str::is_numeric(s: Str) -> Bool                           // all chars are 0-9

pub enum StrError {
    InvalidUtf8  { span: Span },
    OutOfBounds  { index: Int, len: Int, span: Span },
    ParseFailed  { input: Str, span: Span },
}
```

---

## `bytes` — Raw Byte Sequences

`Bytes` is an immutable byte slice. `BytesMut` is a mutable builder.

```rust
// Construction
bytes::new() -> Bytes                                     // empty
bytes::from_hex(s: Str) -> Result<Bytes, BytesError>
bytes::from_base64(s: Str) -> Result<Bytes, BytesError>
bytes::repeat(b: Bytes, n: Int) -> Bytes

// Access
bytes::len(b: Bytes) -> Int
bytes::is_empty(b: Bytes) -> Bool
bytes::get(b: Bytes, index: Int) -> Result<Int, BytesError>  // single byte as Int 0–255
bytes::slice(b: Bytes, from: Int, to: Int) -> Result<Bytes, BytesError>

// Search
bytes::contains(haystack: Bytes, needle: Bytes) -> Bool
bytes::find(haystack: Bytes, needle: Bytes) -> Option<Int>

// Combine
bytes::concat(a: Bytes, b: Bytes) -> Bytes

// Convert
bytes::to_hex(b: Bytes) -> Str
bytes::to_base64(b: Bytes) -> Str
bytes::to_str(b: Bytes) -> Result<Str, BytesError>
bytes::to_list(b: Bytes) -> List<Int>                     // each byte as 0–255 Int
bytes::from_list(ints: List<Int>) -> Result<Bytes, BytesError>  // fails if any > 255

// Mutable builder
bytes::builder() -> BytesMut

bytes::write_byte(buf: BytesMut, byte: Int) -> Unit
bytes::write_bytes(buf: BytesMut, b: Bytes) -> Unit
bytes::write_str(buf: BytesMut, s: Str) -> Unit           // UTF-8 encodes s
bytes::write_u16_le(buf: BytesMut, n: Int) -> Unit
bytes::write_u16_be(buf: BytesMut, n: Int) -> Unit
bytes::write_u32_le(buf: BytesMut, n: Int) -> Unit
bytes::write_u32_be(buf: BytesMut, n: Int) -> Unit
bytes::write_i64_le(buf: BytesMut, n: Int) -> Unit
bytes::write_i64_be(buf: BytesMut, n: Int) -> Unit
bytes::build(buf: BytesMut) -> Bytes                      // consumes the builder

pub enum BytesError {
    OutOfBounds     { index: Int, len: Int, span: Span },
    InvalidHex      { span: Span },
    InvalidBase64   { span: Span },
    InvalidByte     { value: Int, span: Span },           // Int was > 255
    InvalidUtf8     { span: Span },
}
```

---

## `list` — Dynamic Arrays

`List<T>` is Ferric's growable array type. All functions are pure — they return new
lists. In-place mutation lives on `ListMut<T>` for performance-sensitive code.

```rust
// Construction
list::new() -> List<T>
list::of(items: T...) -> List<T>                          // variadic literal helper
list::repeat(item: T, n: Int) -> List<T>
list::range(from: Int, to: Int) -> List<Int>              // exclusive end
list::range_inclusive(from: Int, to: Int) -> List<Int>

// Access
list::len(l: List<T>) -> Int
list::is_empty(l: List<T>) -> Bool
list::get(l: List<T>, index: Int) -> Option<T>
list::first(l: List<T>) -> Option<T>
list::last(l: List<T>) -> Option<T>
list::slice(l: List<T>, from: Int, to: Int) -> List<T>

// Search
list::contains(l: List<T>, item: T) -> Bool              // requires T: Eq (M5+)
list::find(l: List<T>, f: Fn(T) -> Bool) -> Option<T>
list::find_index(l: List<T>, f: Fn(T) -> Bool) -> Option<Int>
list::index_of(l: List<T>, item: T) -> Option<Int>       // requires T: Eq (M5+)

// Transform
list::map(l: List<T>, f: Fn(T) -> U) -> List<U>
list::flat_map(l: List<T>, f: Fn(T) -> List<U>) -> List<U>
list::filter(l: List<T>, f: Fn(T) -> Bool) -> List<T>
list::filter_map(l: List<T>, f: Fn(T) -> Option<U>) -> List<U>
list::reduce(l: List<T>, f: Fn(T, T) -> T) -> Option<T>   // None if empty
list::fold(l: List<T>, init: U, f: Fn(U, T) -> U) -> U
list::scan(l: List<T>, init: U, f: Fn(U, T) -> U) -> List<U>  // fold, collecting intermediate states
list::zip(a: List<T>, b: List<U>) -> List<(T, U)>
list::zip_with(a: List<T>, b: List<U>, f: Fn(T, U) -> V) -> List<V>
list::enumerate(l: List<T>) -> List<(Int, T)>
list::flatten(l: List<List<T>>) -> List<T>
list::chunk(l: List<T>, size: Int) -> List<List<T>>
list::window(l: List<T>, size: Int) -> List<List<T>>       // sliding windows
list::take(l: List<T>, n: Int) -> List<T>
list::drop(l: List<T>, n: Int) -> List<T>
list::take_while(l: List<T>, f: Fn(T) -> Bool) -> List<T>
list::drop_while(l: List<T>, f: Fn(T) -> Bool) -> List<T>
list::partition(l: List<T>, f: Fn(T) -> Bool) -> (List<T>, List<T>)  // (truthy, falsy)
list::unzip(l: List<(T, U)>) -> (List<T>, List<U>)

// Aggregation
list::any(l: List<T>, f: Fn(T) -> Bool) -> Bool
list::all(l: List<T>, f: Fn(T) -> Bool) -> Bool
list::count(l: List<T>, f: Fn(T) -> Bool) -> Int
list::sum(l: List<Int>) -> Int
list::sum_float(l: List<Float>) -> Float
list::min(l: List<T>) -> Option<T>                        // requires T: Ord (M5+)
list::max(l: List<T>) -> Option<T>                        // requires T: Ord (M5+)

// Structural
list::concat(a: List<T>, b: List<T>) -> List<T>
list::prepend(item: T, l: List<T>) -> List<T>
list::append(l: List<T>, item: T) -> List<T>
list::insert(l: List<T>, at: Int, item: T) -> Result<List<T>, ListError>
list::remove(l: List<T>, at: Int) -> Result<List<T>, ListError>
list::reverse(l: List<T>) -> List<T>
list::unique(l: List<T>) -> List<T>                       // requires T: Eq (M5+), preserves order
list::intersperse(l: List<T>, sep: T) -> List<T>          // [1,2,3] → [1,sep,2,sep,3]
list::join_str(l: List<Str>, sep: Str) -> Str             // convenience — same as str::join

// Mutable builder (for hot paths)
list::builder() -> ListMut<T>
list::push(buf: ListMut<T>, item: T) -> Unit
list::pop(buf: ListMut<T>) -> Option<T>
list::build(buf: ListMut<T>) -> List<T>

pub enum ListError {
    OutOfBounds { index: Int, len: Int, span: Span },
    InvalidChunkSize { size: Int, span: Span },
}
```

---

## `map` — Hash Maps

`Map<K, V>` is Ferric's hash map. Immutable by default; `MapMut<K, V>` for builders.

```rust
// Construction
map::new() -> Map<K, V>
map::from_list(pairs: List<(K, V)>) -> Map<K, V>          // last write wins on dup keys
map::from_lists(keys: List<K>, vals: List<V>) -> Result<Map<K, V>, MapError>

// Access
map::get(m: Map<K, V>, key: K) -> Option<V>
map::get_or(m: Map<K, V>, key: K, default: V) -> V
map::contains_key(m: Map<K, V>, key: K) -> Bool
map::len(m: Map<K, V>) -> Int
map::is_empty(m: Map<K, V>) -> Bool

// Modification (returns new map)
map::insert(m: Map<K, V>, key: K, val: V) -> Map<K, V>
map::remove(m: Map<K, V>, key: K) -> Map<K, V>            // no-op if absent
map::merge(a: Map<K, V>, b: Map<K, V>) -> Map<K, V>       // b wins on conflict
map::merge_with(a: Map<K, V>, b: Map<K, V>, f: Fn(V, V) -> V) -> Map<K, V>

// Views
map::keys(m: Map<K, V>) -> List<K>
map::values(m: Map<K, V>) -> List<V>
map::entries(m: Map<K, V>) -> List<(K, V)>

// Transform
map::map_values(m: Map<K, V>, f: Fn(V) -> W) -> Map<K, W>
map::filter(m: Map<K, V>, f: Fn(K, V) -> Bool) -> Map<K, V>
map::filter_map(m: Map<K, V>, f: Fn(K, V) -> Option<W>) -> Map<K, W>
map::fold(m: Map<K, V>, init: U, f: Fn(U, K, V) -> U) -> U

// Grouping (extremely useful, not in enough stdlibs)
map::group_by(l: List<T>, f: Fn(T) -> K) -> Map<K, List<T>>
map::count_by(l: List<T>, f: Fn(T) -> K) -> Map<K, Int>

// Mutable builder
map::builder() -> MapMut<K, V>
map::set(buf: MapMut<K, V>, key: K, val: V) -> Unit
map::build(buf: MapMut<K, V>) -> Map<K, V>

pub enum MapError {
    LengthMismatch { keys: Int, vals: Int, span: Span },
}
```

---

## `set` — Hash Sets

```rust
// Construction
set::new() -> Set<T>
set::from_list(l: List<T>) -> Set<T>

// Membership
set::contains(s: Set<T>, item: T) -> Bool
set::len(s: Set<T>) -> Int
set::is_empty(s: Set<T>) -> Bool

// Modification (returns new set)
set::insert(s: Set<T>, item: T) -> Set<T>
set::remove(s: Set<T>, item: T) -> Set<T>

// Set algebra
set::union(a: Set<T>, b: Set<T>) -> Set<T>
set::intersection(a: Set<T>, b: Set<T>) -> Set<T>
set::difference(a: Set<T>, b: Set<T>) -> Set<T>            // a - b
set::symmetric_difference(a: Set<T>, b: Set<T>) -> Set<T>
set::is_subset(a: Set<T>, b: Set<T>) -> Bool
set::is_superset(a: Set<T>, b: Set<T>) -> Bool
set::is_disjoint(a: Set<T>, b: Set<T>) -> Bool

// Convert
set::to_list(s: Set<T>) -> List<T>                         // unspecified order

// Transform
set::filter(s: Set<T>, f: Fn(T) -> Bool) -> Set<T>
set::map(s: Set<T>, f: Fn(T) -> U) -> Set<U>
```

---

## `sort` — Sorting and Ordering

Sorting is explicit and composable. No magic `Comparable` interfaces.

```rust
// Basic sort (requires T: Ord, M5+)
sort::asc(l: List<T>) -> List<T>
sort::desc(l: List<T>) -> List<T>

// Key-based sort (works before M5)
sort::by(l: List<T>, key: Fn(T) -> K) -> List<T>          // ascending by key
sort::by_desc(l: List<T>, key: Fn(T) -> K) -> List<T>     // descending by key

// Comparator-based sort
sort::with(l: List<T>, cmp: Fn(T, T) -> Ordering) -> List<T>

// Stable sort (all variants are stable — this is the only kind Ferric offers)
// Unstable sort would be a separate sort::unstable:: namespace if ever needed

// Ordering enum (used in comparator functions)
pub enum Ordering { Less, Equal, Greater }

// Comparator helpers
sort::cmp_int(a: Int, b: Int) -> Ordering
sort::cmp_float(a: Float, b: Float) -> Ordering
sort::cmp_str(a: Str, b: Str) -> Ordering                  // lexicographic
sort::cmp_str_natural(a: Str, b: Str) -> Ordering          // "file10" > "file9"
sort::reverse(cmp: Fn(T, T) -> Ordering) -> Fn(T, T) -> Ordering
sort::then(cmp: Fn(T, T) -> Ordering, tiebreak: Fn(T, T) -> Ordering) -> Fn(T, T) -> Ordering

// Partial sort
sort::top(l: List<T>, n: Int, key: Fn(T) -> K) -> List<T>  // n largest by key
sort::bottom(l: List<T>, n: Int, key: Fn(T) -> K) -> List<T>

// Check
sort::is_sorted(l: List<T>) -> Bool                         // requires T: Ord (M5+)
sort::is_sorted_by(l: List<T>, key: Fn(T) -> K) -> Bool
```

---

## `math` — Numeric Operations

```rust
// Integer
math::abs(n: Int) -> Int
math::min(a: Int, b: Int) -> Int
math::max(a: Int, b: Int) -> Int
math::clamp(n: Int, low: Int, high: Int) -> Int
math::pow(base: Int, exp: Int) -> Result<Int, MathError>   // fails on negative exp
math::gcd(a: Int, b: Int) -> Int
math::lcm(a: Int, b: Int) -> Int
math::div_floor(a: Int, b: Int) -> Result<Int, MathError>  // fails on b=0
math::div_ceil(a: Int, b: Int) -> Result<Int, MathError>
math::rem_euclean(a: Int, b: Int) -> Int                   // always non-negative

// Float
math::abs_f(n: Float) -> Float
math::min_f(a: Float, b: Float) -> Float
math::max_f(a: Float, b: Float) -> Float
math::clamp_f(n: Float, low: Float, high: Float) -> Float
math::floor(n: Float) -> Float
math::ceil(n: Float) -> Float
math::round(n: Float) -> Float
math::trunc(n: Float) -> Float
math::fract(n: Float) -> Float                              // fractional part
math::sqrt(n: Float) -> Result<Float, MathError>           // fails on negative
math::cbrt(n: Float) -> Float
math::pow_f(base: Float, exp: Float) -> Float
math::log(n: Float, base: Float) -> Result<Float, MathError>
math::ln(n: Float) -> Result<Float, MathError>
math::log2(n: Float) -> Result<Float, MathError>
math::log10(n: Float) -> Result<Float, MathError>
math::exp(n: Float) -> Float
math::sin(n: Float) -> Float
math::cos(n: Float) -> Float
math::tan(n: Float) -> Float
math::asin(n: Float) -> Result<Float, MathError>
math::acos(n: Float) -> Result<Float, MathError>
math::atan(n: Float) -> Float
math::atan2(y: Float, x: Float) -> Float
math::hypot(a: Float, b: Float) -> Float
math::is_nan(n: Float) -> Bool
math::is_finite(n: Float) -> Bool
math::is_inf(n: Float) -> Bool

// Constants
math::PI  -> Float
math::E   -> Float
math::TAU -> Float
math::INF -> Float
math::NAN -> Float

// Conversion
math::int_to_float(n: Int) -> Float
math::float_to_int(n: Float) -> Int                        // truncates toward zero

pub enum MathError {
    DivisionByZero     { span: Span },
    NegativeSqrt       { value: Float, span: Span },
    LogOfNonPositive   { value: Float, span: Span },
    Overflow           { span: Span },
}
```

---

## `io` — File I/O and Standard Streams

```rust
// Standard streams
io::print(s: Str) -> Unit
io::println(s: Str) -> Unit
io::eprint(s: Str) -> Unit                                 // stderr
io::eprintln(s: Str) -> Unit
io::read_line() -> Result<Str, IoError>                    // reads one line from stdin
io::read_all() -> Result<Str, IoError>                     // reads all of stdin

// File — text
io::read_file(path: Str) -> Result<Str, IoError>
io::write_file(path: Str, content: Str) -> Result<Unit, IoError>
io::append_file(path: Str, content: Str) -> Result<Unit, IoError>
io::read_lines(path: Str) -> Result<List<Str>, IoError>    // reads file, splits on newlines

// File — bytes
io::read_bytes(path: Str) -> Result<Bytes, IoError>
io::write_bytes(path: Str, content: Bytes) -> Result<Unit, IoError>

// File metadata
io::exists(path: Str) -> Bool
io::is_file(path: Str) -> Bool
io::is_dir(path: Str) -> Bool
io::file_size(path: Str) -> Result<Int, IoError>           // bytes
io::modified_at(path: Str) -> Result<Time, IoError>

// Directory
io::list_dir(path: Str) -> Result<List<Str>, IoError>      // returns filenames, not full paths
io::list_dir_recursive(path: Str) -> Result<List<Str>, IoError>
io::make_dir(path: Str) -> Result<Unit, IoError>
io::make_dir_all(path: Str) -> Result<Unit, IoError>
io::remove_file(path: Str) -> Result<Unit, IoError>
io::remove_dir(path: Str) -> Result<Unit, IoError>         // must be empty
io::remove_dir_all(path: Str) -> Result<Unit, IoError>
io::rename(from: Str, to: Str) -> Result<Unit, IoError>
io::copy(from: Str, to: Str) -> Result<Int, IoError>       // returns bytes copied

// Buffered writer (for writing many small strings efficiently)
io::file_writer(path: Str) -> Result<FileWriter, IoError>
io::write(w: FileWriter, s: Str) -> Result<Unit, IoError>
io::writeln(w: FileWriter, s: Str) -> Result<Unit, IoError>
io::flush(w: FileWriter) -> Result<Unit, IoError>
io::close(w: FileWriter) -> Result<Unit, IoError>          // flushes then closes

pub enum IoError {
    NotFound          { path: Str, span: Span },
    PermissionDenied  { path: Str, span: Span },
    AlreadyExists     { path: Str, span: Span },
    IsDirectory       { path: Str, span: Span },
    IsFile            { path: Str, span: Span },
    NotEmpty          { path: Str, span: Span },
    Interrupted       { span: Span },
    Other             { message: Str, span: Span },
}
```

---

## `path` — Filesystem Path Manipulation

Paths are represented as `Str` at the surface level. No `Path` type — that
complexity belongs in a future `std::fs` expansion. These functions do string
manipulation according to OS path rules, with no I/O.

```rust
path::join(base: Str, parts: Str...) -> Str
path::parent(p: Str) -> Option<Str>
path::filename(p: Str) -> Option<Str>                      // last component
path::stem(p: Str) -> Option<Str>                          // filename without extension
path::extension(p: Str) -> Option<Str>                     // e.g. "rs" from "main.rs"
path::with_extension(p: Str, ext: Str) -> Str
path::is_absolute(p: Str) -> Bool
path::is_relative(p: Str) -> Bool
path::normalize(p: Str) -> Str                             // resolves . and .. lexically
path::relative_to(p: Str, base: Str) -> Result<Str, PathError>
path::components(p: Str) -> List<Str>

pub enum PathError {
    NotRelativeTo { path: Str, base: Str, span: Span },
}
```

---

## `env` — Environment Variables and Process

```rust
env::get(name: Str) -> Option<Str>
env::get_or(name: Str, default: Str) -> Str
env::require(name: Str) -> Result<Str, EnvError>           // error if absent
env::set(name: Str, val: Str) -> Unit                      // sets for current process
env::remove(name: Str) -> Unit
env::all() -> Map<Str, Str>                                // all env vars as a map

env::args() -> List<Str>                                   // process argv
env::cwd() -> Result<Str, IoError>
env::home_dir() -> Option<Str>
env::temp_dir() -> Str

env::exit(code: Int) -> Unit                               // terminates process, never returns

pub enum EnvError {
    NotSet { name: Str, span: Span },
}
```

---

## `http` — HTTP Client

Ferric's HTTP client is fetch-inspired but typed. Every response carries a status
code and typed body — no automatic throws on non-2xx.

```rust
// Quick helpers
http::get(url: Str) -> Result<Response, HttpError>
http::post(url: Str, body: Bytes) -> Result<Response, HttpError>
http::put(url: Str, body: Bytes) -> Result<Response, HttpError>
http::patch(url: Str, body: Bytes) -> Result<Response, HttpError>
http::delete(url: Str) -> Result<Response, HttpError>

// Full request builder
http::request(method: Str, url: Str) -> RequestBuilder

// RequestBuilder methods — returns self for chaining (or use named params)
http::header(req: RequestBuilder, name: Str, val: Str) -> RequestBuilder
http::headers(req: RequestBuilder, h: Map<Str, Str>) -> RequestBuilder
http::body_bytes(req: RequestBuilder, b: Bytes) -> RequestBuilder
http::body_str(req: RequestBuilder, s: Str) -> RequestBuilder
http::body_json(req: RequestBuilder, val: Json) -> RequestBuilder
http::timeout(req: RequestBuilder, ms: Int) -> RequestBuilder
http::follow_redirects(req: RequestBuilder, follow: Bool = true) -> RequestBuilder
http::basic_auth(req: RequestBuilder, user: Str, pass: Str) -> RequestBuilder
http::bearer(req: RequestBuilder, token: Str) -> RequestBuilder
http::send(req: RequestBuilder) -> Result<Response, HttpError>

// Response inspection
http::status(r: Response) -> Int                           // HTTP status code
http::ok(r: Response) -> Bool                              // status is 2xx
http::header_get(r: Response, name: Str) -> Option<Str>
http::headers_all(r: Response) -> Map<Str, Str>
http::body_bytes(r: Response) -> Bytes
http::body_str(r: Response) -> Result<Str, StrError>
http::body_json(r: Response) -> Result<Json, JsonError>

// Typed convenience
http::get_json(url: Str) -> Result<Json, HttpError>
http::post_json(url: Str, body: Json) -> Result<Json, HttpError>

pub enum HttpError {
    InvalidUrl        { url: Str, span: Span },
    ConnectionFailed  { url: Str, reason: Str, span: Span },
    Timeout           { url: Str, ms: Int, span: Span },
    TlsError          { reason: Str, span: Span },
    RedirectLoop      { url: Str, span: Span },
    InvalidResponse   { reason: Str, span: Span },
}
```

---

## `serve` — HTTP Server

Ferric's HTTP server is Fastify-inspired: fast, layered, and declarative.
Routes are defined before the server starts; there's no mutable global router.

```rust
// Create a server
serve::new() -> Server

// Route registration (returns server for builder-style chaining)
serve::get(s: Server, path: Str, handler: Fn(Request) -> Response) -> Server
serve::post(s: Server, path: Str, handler: Fn(Request) -> Response) -> Server
serve::put(s: Server, path: Str, handler: Fn(Request) -> Response) -> Server
serve::patch(s: Server, path: Str, handler: Fn(Request) -> Response) -> Server
serve::delete(s: Server, path: Str, handler: Fn(Request) -> Response) -> Server
serve::any(s: Server, path: Str, handler: Fn(Request) -> Response) -> Server

// Middleware — runs before every handler on this server (or a prefix)
serve::use(s: Server, m: Fn(Request, Fn(Request) -> Response) -> Response) -> Server
serve::use_prefix(s: Server, prefix: Str, m: Fn(Request, Fn(Request) -> Response) -> Response) -> Server

// Sub-router (namespaced routes)
serve::mount(s: Server, prefix: Str, sub: Server) -> Server

// Start listening
serve::listen(s: Server, port: Int) -> Result<Unit, ServeError>
serve::listen_addr(s: Server, addr: Str, port: Int) -> Result<Unit, ServeError>

// Request inspection
serve::method(r: Request) -> Str
serve::path(r: Request) -> Str
serve::query(r: Request) -> Map<Str, Str>
serve::param(r: Request, name: Str) -> Option<Str>         // path params from :name
serve::header_get(r: Request, name: Str) -> Option<Str>
serve::body_bytes(r: Request) -> Bytes
serve::body_str(r: Request) -> Result<Str, StrError>
serve::body_json(r: Request) -> Result<Json, JsonError>

// Response building
serve::response(status: Int, body: Bytes) -> Response
serve::ok(body: Bytes) -> Response                         // 200
serve::ok_str(body: Str) -> Response                       // 200 + text/plain
serve::ok_json(body: Json) -> Response                     // 200 + application/json
serve::created(body: Json) -> Response                     // 201
serve::no_content() -> Response                            // 204
serve::bad_request(message: Str) -> Response               // 400 + JSON error body
serve::unauthorized(message: Str) -> Response              // 401
serve::forbidden(message: Str) -> Response                 // 403
serve::not_found(message: Str) -> Response                 // 404
serve::internal_error(message: Str) -> Response            // 500
serve::with_header(r: Response, name: Str, val: Str) -> Response
serve::with_headers(r: Response, h: Map<Str, Str>) -> Response

pub enum ServeError {
    PortInUse         { port: Int, span: Span },
    PermissionDenied  { port: Int, span: Span },
    InvalidAddress    { addr: Str, span: Span },
}
```

### Example

```rust
let server = serve::new()
    |> serve::get(path: "/health", handler: |_req| serve::ok_str(body: "ok"))
    |> serve::post(path: "/echo", handler: |req| {
        let body = serve::body_str(r: req)?
        serve::ok_str(body: body)
    })

serve::listen(s: server, port: 8080)?
```

---

## `sock` — Sockets

TCP and UDP. Low-level but not painful.

```rust
// TCP client
sock::tcp_connect(host: Str, port: Int) -> Result<TcpStream, SockError>
sock::tcp_connect_timeout(host: Str, port: Int, ms: Int) -> Result<TcpStream, SockError>

// TcpStream operations
sock::read(s: TcpStream, buf: BytesMut) -> Result<Int, SockError>  // returns bytes read
sock::read_exact(s: TcpStream, n: Int) -> Result<Bytes, SockError>
sock::read_line(s: TcpStream) -> Result<Str, SockError>            // reads until \n
sock::write(s: TcpStream, data: Bytes) -> Result<Int, SockError>   // returns bytes written
sock::write_all(s: TcpStream, data: Bytes) -> Result<Unit, SockError>
sock::write_str(s: TcpStream, data: Str) -> Result<Unit, SockError>
sock::flush(s: TcpStream) -> Result<Unit, SockError>
sock::close(s: TcpStream) -> Result<Unit, SockError>
sock::set_timeout(s: TcpStream, ms: Int) -> Result<Unit, SockError>
sock::local_addr(s: TcpStream) -> Result<Str, SockError>
sock::peer_addr(s: TcpStream) -> Result<Str, SockError>

// TCP server
sock::tcp_listen(port: Int) -> Result<TcpListener, SockError>
sock::tcp_listen_addr(addr: Str, port: Int) -> Result<TcpListener, SockError>
sock::accept(l: TcpListener) -> Result<TcpStream, SockError>
sock::accept_loop(l: TcpListener, handler: Fn(TcpStream) -> Unit) -> Result<Unit, SockError>

// UDP
sock::udp_bind(port: Int) -> Result<UdpSocket, SockError>
sock::udp_bind_addr(addr: Str, port: Int) -> Result<UdpSocket, SockError>
sock::udp_send(s: UdpSocket, to: Str, port: Int, data: Bytes) -> Result<Int, SockError>
sock::udp_recv(s: UdpSocket) -> Result<(Bytes, Str), SockError>    // (data, sender_addr)

pub enum SockError {
    ConnectionRefused  { host: Str, port: Int, span: Span },
    ConnectionReset    { span: Span },
    Timeout            { ms: Int, span: Span },
    AddressInUse       { port: Int, span: Span },
    InvalidAddress     { addr: Str, span: Span },
    Disconnected       { span: Span },
    Other              { message: Str, span: Span },
}
```

---

## `crypto` — Cryptography

Opinionated: we expose primitives that are safe to use, not every possible cipher.
Dangerous patterns (raw ECB, MD5 as a security primitive, etc.) are absent by design.

```rust
// Hashing (non-keyed)
crypto::sha256(data: Bytes) -> Bytes
crypto::sha384(data: Bytes) -> Bytes
crypto::sha512(data: Bytes) -> Bytes
crypto::blake3(data: Bytes) -> Bytes

// Convenience — string input hashes the UTF-8 encoding
crypto::sha256_str(s: Str) -> Bytes
crypto::sha512_str(s: Str) -> Bytes
crypto::blake3_str(s: Str) -> Bytes

// HMAC
crypto::hmac_sha256(key: Bytes, data: Bytes) -> Bytes
crypto::hmac_sha512(key: Bytes, data: Bytes) -> Bytes

// Constant-time comparison (prevents timing attacks)
crypto::eq_constant(a: Bytes, b: Bytes) -> Bool

// Symmetric encryption — ChaCha20-Poly1305 (authenticated)
crypto::chacha_encrypt(key: Bytes, nonce: Bytes, plaintext: Bytes) -> Result<Bytes, CryptoError>
crypto::chacha_decrypt(key: Bytes, nonce: Bytes, ciphertext: Bytes) -> Result<Bytes, CryptoError>
crypto::chacha_key() -> Bytes                              // generates a random 32-byte key
crypto::chacha_nonce() -> Bytes                            // generates a random 12-byte nonce

// AES-GCM (for interoperability with existing systems)
crypto::aes_gcm_encrypt(key: Bytes, nonce: Bytes, plaintext: Bytes) -> Result<Bytes, CryptoError>
crypto::aes_gcm_decrypt(key: Bytes, nonce: Bytes, ciphertext: Bytes) -> Result<Bytes, CryptoError>

// Password hashing (Argon2id — the right default)
crypto::hash_password(password: Str) -> Result<Str, CryptoError>          // stores params + salt in output
crypto::verify_password(password: Str, hash: Str) -> Result<Bool, CryptoError>

// Asymmetric — Ed25519 signing
crypto::ed25519_keypair() -> (Bytes, Bytes)                // (private_key, public_key)
crypto::ed25519_sign(private_key: Bytes, message: Bytes) -> Result<Bytes, CryptoError>
crypto::ed25519_verify(public_key: Bytes, message: Bytes, sig: Bytes) -> Result<Bool, CryptoError>

// Secure random bytes
crypto::random_bytes(n: Int) -> Bytes                      // cryptographically secure
crypto::random_int(low: Int, high: Int) -> Int             // inclusive bounds, crypto-secure

pub enum CryptoError {
    InvalidKeyLength    { expected: Int, found: Int, span: Span },
    InvalidNonceLength  { expected: Int, found: Int, span: Span },
    DecryptionFailed    { span: Span },                    // authentication tag mismatch
    InvalidHashFormat   { span: Span },
    InternalError       { message: Str, span: Span },
}
```

---

## `encode` — Encoding / Decoding

```rust
// Base64
encode::base64(data: Bytes) -> Str
encode::base64_url(data: Bytes) -> Str                     // URL-safe variant, no padding
encode::base64_decode(s: Str) -> Result<Bytes, EncodeError>
encode::base64_url_decode(s: Str) -> Result<Bytes, EncodeError>

// Hex
encode::hex(data: Bytes) -> Str                            // lowercase
encode::hex_upper(data: Bytes) -> Str
encode::hex_decode(s: Str) -> Result<Bytes, EncodeError>

// URL encoding (percent encoding)
encode::url_encode(s: Str) -> Str
encode::url_decode(s: Str) -> Result<Str, EncodeError>
encode::url_encode_component(s: Str) -> Str               // encodes more aggressively
encode::url_decode_component(s: Str) -> Result<Str, EncodeError>

// HTML entities
encode::html_escape(s: Str) -> Str                        // & < > " ' → &amp; etc.
encode::html_unescape(s: Str) -> Str

// CSV (simple — for complex CSV, use the csv package)
encode::csv_row(fields: List<Str>) -> Str
encode::csv_parse_row(s: Str) -> List<Str>

pub enum EncodeError {
    InvalidBase64     { span: Span },
    InvalidHex        { span: Span },
    InvalidPercent    { input: Str, span: Span },
    InvalidUtf8       { span: Span },
}
```

---

## `json` — JSON

A typed JSON value tree. Ferric does not have runtime reflection, so JSON is
explicitly typed through the `Json` enum.

```rust
pub enum Json {
    Null,
    Bool(Bool),
    Int(Int),
    Float(Float),
    Str(Str),
    Array(List<Json>),
    Object(Map<Str, Json>),
}

// Serialise
json::to_str(v: Json) -> Str
json::to_str_pretty(v: Json) -> Str

// Parse
json::parse(s: Str) -> Result<Json, JsonError>

// Access helpers (return None rather than panicking)
json::as_bool(v: Json) -> Option<Bool>
json::as_int(v: Json) -> Option<Int>
json::as_float(v: Json) -> Option<Float>
json::as_str(v: Json) -> Option<Str>
json::as_array(v: Json) -> Option<List<Json>>
json::as_object(v: Json) -> Option<Map<Str, Json>>
json::is_null(v: Json) -> Bool

// Object field access
json::get(v: Json, key: Str) -> Option<Json>
json::get_path(v: Json, path: List<Str>) -> Option<Json>   // nested access

// Construction helpers
json::null() -> Json
json::bool(b: Bool) -> Json
json::int(n: Int) -> Json
json::float(n: Float) -> Json
json::str(s: Str) -> Json
json::array(items: List<Json>) -> Json
json::object(fields: List<(Str, Json)>) -> Json

pub enum JsonError {
    SyntaxError    { message: Str, line: Int, col: Int, span: Span },
    UnexpectedEof  { span: Span },
    TrailingData   { span: Span },
}
```

---

## `time` — Timestamps and Durations

```rust
// Current time
time::now() -> Time                                        // wall clock, UTC
time::now_local() -> Time                                  // with system timezone
time::monotonic() -> Int                                   // nanoseconds since process start

// Construction
time::from_unix(secs: Int) -> Time
time::from_unix_ms(ms: Int) -> Time
time::from_unix_ns(ns: Int) -> Time

// Decomposition
time::unix(t: Time) -> Int
time::unix_ms(t: Time) -> Int
time::year(t: Time) -> Int
time::month(t: Time) -> Int                                // 1–12
time::day(t: Time) -> Int                                  // 1–31
time::hour(t: Time) -> Int
time::minute(t: Time) -> Int
time::second(t: Time) -> Int
time::weekday(t: Time) -> Int                              // 0=Monday … 6=Sunday

// Arithmetic
time::add_secs(t: Time, secs: Int) -> Time
time::add_ms(t: Time, ms: Int) -> Time
time::add_days(t: Time, days: Int) -> Time
time::diff_secs(a: Time, b: Time) -> Int                   // a - b
time::diff_ms(a: Time, b: Time) -> Int

// Formatting
time::format(t: Time, layout: Str) -> Str                  // strftime-style layout
time::format_iso(t: Time) -> Str                           // ISO 8601
time::format_rfc2822(t: Time) -> Str                       // email-compatible
time::parse(s: Str, layout: Str) -> Result<Time, TimeError>
time::parse_iso(s: Str) -> Result<Time, TimeError>

// Duration type (for intervals, timeouts, etc.)
time::secs(n: Int) -> Duration
time::ms(n: Int) -> Duration
time::minutes(n: Int) -> Duration
time::hours(n: Int) -> Duration
time::days(n: Int) -> Duration
time::sleep(d: Duration) -> Unit                           // blocks current thread

pub enum TimeError {
    InvalidLayout  { layout: Str, span: Span },
    ParseFailed    { input: Str, span: Span },
    OutOfRange     { span: Span },
}
```

---

## `rand` — Random Numbers

Non-cryptographic, seeded, reproducible.

```rust
// Unseeded (different each run)
rand::int(low: Int, high: Int) -> Int                      // inclusive bounds
rand::float() -> Float                                     // [0.0, 1.0)
rand::bool() -> Bool
rand::pick<T>(l: List<T>) -> Option<T>
rand::shuffle<T>(l: List<T>) -> List<T>
rand::sample<T>(l: List<T>, n: Int) -> List<T>            // n items without replacement

// Seeded (reproducible)
rand::seeded(seed: Int) -> Rng
rand::rng_int(r: Rng, low: Int, high: Int) -> (Int, Rng)   // returns new Rng for functional use
rand::rng_float(r: Rng) -> (Float, Rng)
rand::rng_pick<T>(r: Rng, l: List<T>) -> (Option<T>, Rng)
rand::rng_shuffle<T>(r: Rng, l: List<T>) -> (List<T>, Rng)
```

---

## `re` — Regular Expressions

Compiled regexes are values. Patterns are compiled once, reused many times.

```rust
// Compilation
re::compile(pattern: Str) -> Result<Regex, ReError>
re::compile_flags(pattern: Str, flags: Str) -> Result<Regex, ReError>  // flags: "im" etc.

// Matching
re::is_match(r: Regex, s: Str) -> Bool
re::find(r: Regex, s: Str) -> Option<Match>
re::find_all(r: Regex, s: Str) -> List<Match>

// Match inspection
re::match_str(m: Match) -> Str                             // full match text
re::match_start(m: Match) -> Int
re::match_end(m: Match) -> Int
re::capture(m: Match, index: Int) -> Option<Str>           // capture group by index
re::capture_name(m: Match, name: Str) -> Option<Str>       // named capture group

// Replace
re::replace(r: Regex, s: Str, with: Str) -> Str            // replaces first match
re::replace_all(r: Regex, s: Str, with: Str) -> Str
re::replace_fn(r: Regex, s: Str, f: Fn(Match) -> Str) -> Str  // dynamic replacement

// Split
re::split(r: Regex, s: Str) -> List<Str>
re::split_n(r: Regex, s: Str, n: Int) -> List<Str>

pub enum ReError {
    InvalidPattern   { pattern: Str, message: Str, span: Span },
    InvalidFlags     { flags: Str, span: Span },
}
```

---

## `fmt` — Formatting

String formatting beyond `+` concatenation.

```rust
// Printf-style formatting
fmt::format(template: Str, args: List<Str>) -> Str         // {} as placeholder
// e.g. fmt::format(template: "hello, {}!", args: ["world"]) → "hello, world!"

// Typed formatting
fmt::int(n: Int, radix: Int = 10) -> Str                   // radix 2, 8, 10, or 16
fmt::int_padded(n: Int, width: Int, pad: Str = "0") -> Str
fmt::float(n: Float, decimals: Int) -> Str
fmt::float_exp(n: Float, decimals: Int) -> Str             // scientific notation
fmt::bool(b: Bool) -> Str                                  // "true" / "false"

// Debug-style printing (returns a representation, never panics)
fmt::debug(v: Json) -> Str                                 // pretty-print a Json value
```

---

## `proc` — Subprocess Management

Beyond the `$` shell expression (which is fire-and-forget capture), `proc` gives
full control over subprocess lifecycle.

```rust
// Simple capture (same as $ but in a function call)
proc::run(cmd: Str) -> Result<ProcOutput, ProcError>
proc::run_shell(cmd: Str) -> Result<ProcOutput, ProcError>  // via sh -c

// Output inspection
proc::stdout(o: ProcOutput) -> Str
proc::stderr(o: ProcOutput) -> Str
proc::exit_code(o: ProcOutput) -> Int
proc::ok(o: ProcOutput) -> Bool                            // exit code is 0

// Full process builder
proc::command(program: Str) -> Command
proc::arg(c: Command, a: Str) -> Command
proc::args(c: Command, a: List<Str>) -> Command
proc::env_set(c: Command, key: Str, val: Str) -> Command
proc::env_clear(c: Command) -> Command                     // start with empty environment
proc::cwd(c: Command, path: Str) -> Command
proc::stdin_bytes(c: Command, data: Bytes) -> Command      // pipe data to stdin
proc::stdin_str(c: Command, data: Str) -> Command
proc::timeout(c: Command, ms: Int) -> Command
proc::spawn(c: Command) -> Result<ProcOutput, ProcError>   // runs and waits

// Streaming subprocess (for long-running processes)
proc::spawn_streaming(c: Command) -> Result<Process, ProcError>
proc::write_stdin(p: Process, data: Bytes) -> Result<Unit, ProcError>
proc::read_stdout_line(p: Process) -> Result<Str, ProcError>
proc::wait(p: Process) -> Result<Int, ProcError>           // waits for exit, returns code
proc::kill(p: Process) -> Result<Unit, ProcError>

pub enum ProcError {
    NotFound           { program: Str, span: Span },
    PermissionDenied   { program: Str, span: Span },
    Timeout            { ms: Int, span: Span },
    Other              { message: Str, span: Span },
}
```

---

## `log` — Structured Logging

Opinionated: logs are structured and go to stderr. No log files, no global logger,
no config file. Filtering is by level; levels are comparable integers.

```rust
// Log a message at a given level
log::debug(msg: Str) -> Unit
log::info(msg: Str) -> Unit
log::warn(msg: Str) -> Unit
log::error(msg: Str) -> Unit

// Structured — attach key-value pairs
log::debug_fields(msg: Str, fields: Map<Str, Str>) -> Unit
log::info_fields(msg: Str, fields: Map<Str, Str>) -> Unit
log::warn_fields(msg: Str, fields: Map<Str, Str>) -> Unit
log::error_fields(msg: Str, fields: Map<Str, Str>) -> Unit

// Logger — scoped, carries a fixed set of fields
log::new(fields: Map<Str, Str>) -> Logger
log::with(l: Logger, key: Str, val: Str) -> Logger         // returns new logger with extra field
log::log_debug(l: Logger, msg: Str) -> Unit
log::log_info(l: Logger, msg: Str) -> Unit
log::log_warn(l: Logger, msg: Str) -> Unit
log::log_error(l: Logger, msg: Str) -> Unit

// Level control (affects process globally)
log::set_level(level: LogLevel) -> Unit
log::get_level() -> LogLevel

pub enum LogLevel { Debug, Info, Warn, Error }

// Default output format (to stderr):
// 2026-04-25T14:30:00Z INFO  message  key=val key=val
// Controlled by LOG_FORMAT env var: "pretty" (default) or "json"
```

---

## `sync` — Channels and Mutexes *(async milestone, post-M3)*

This module is a stub in M6 and becomes available after the async runtime lands.

```rust
// Channel — typed, bounded or unbounded
sync::channel<T>(capacity: Int = 0) -> (Sender<T>, Receiver<T>)   // 0 = unbounded
sync::send(s: Sender<T>, val: T) -> Result<Unit, SyncError>
sync::recv(r: Receiver<T>) -> Result<T, SyncError>
sync::try_recv(r: Receiver<T>) -> Option<T>

// Mutex
sync::mutex<T>(val: T) -> Mutex<T>
sync::lock(m: Mutex<T>, f: Fn(T) -> (T, U)) -> U             // lock, transform, unlock

// Once cell — initialise once, read forever
sync::once<T>(init: Fn() -> T) -> Once<T>
sync::get(o: Once<T>) -> T

pub enum SyncError {
    Disconnected { span: Span },                           // sender/receiver dropped
    Poisoned     { span: Span },                           // lock held by panicked thread
}
```

---

## Naming Conventions (Summary)

| Convention | Example |
|---|---|
| Functions are snake_case | `str::trim_start`, `http::body_json` |
| Error enums are PascalCase variants | `HttpError::Timeout` |
| Builder functions return the builder | `serve::get` returns `Server` |
| Predicate functions prefix `is_` | `str::is_empty`, `sock::ok` |
| Conversion functions prefix `to_` or `from_` | `str::to_bytes`, `bytes::from_hex` |
| Destructive/consuming operations suffix `_all` when paired with partial ops | `io::remove_dir_all` vs `io::remove_dir` |
| Mutable variants of types suffix `Mut` | `BytesMut`, `ListMut`, `MapMut` |

---

## What Is Deliberately Absent

**No XML.** Use JSON. If you need XML, write a `ferric-xml` package.

**No ORM.** Database clients are ecosystem packages (`ferric-postgres`, `ferric-sqlite`).
The stdlib ships no database drivers.

**No async runtime in the stdlib.** The `sync` module stubs the interface;
the runtime is the VM's concern. No `Promise`, no `Future` in stdlib types.

**No global mutable state in any module.** Every module function is a pure
function or takes explicit state as an argument. `rand::seeded` and `log::new`
make this possible without globals.

**No deprecated functions.** Once something is in the stdlib it is stable.
If a better design is found, it goes into a new function name and the old
one is documented with a migration path — never silently broken.