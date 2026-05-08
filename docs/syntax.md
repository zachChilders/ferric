# M10 — Ferric Identity

> This milestone gives Ferric a distinct syntactic identity. All changes are
> motivated by ergonomics, readability, and safety. No existing semantics are
> altered — this milestone is purely surface syntax and calling convention.
> 
> Tasks must be completed in order. Task 1 (common types) must compile cleanly
> before any other task begins. Tasks 2–6 may then proceed in parallel.

-----

## Overview of changes

|Task|Change                                                                      |
|----|----------------------------------------------------------------------------|
|1   |Common types — new AST nodes for all features below                         |
|2   |F-string interpolation (f"...")                                           |
|3   |Pipeline operator (|>)                                                    |
|4   |? propagation and must unwrap                                           |
|5   |Labeled breaks ('label:) and destructuring let                          |
|6   |Implicit named args, method syntax on stdlib types, opaque-only type aliases|

-----

## Task 1: Common Types

> **Do this task first.** All other tasks depend on the AST nodes added here.
> Nothing is wired up yet — this task only extends data structures.
> Tasks 2–6 may not begin until this task is complete and the codebase compiles
> cleanly.

### 1. FStringExpr

F-strings are a new expression form. A parsed f-string is a sequence of
alternating literal segments and interpolated expressions.

pub struct FStringSegment {
    pub span: Span,
    pub kind: FStringSegmentKind,
}

pub enum FStringSegmentKind {
    Lit(String),          // literal text between braces
    Expr(Box<Expr>),      // {expr} — any Ferric expression
}

pub struct FStringExpr {
    pub span:     Span,
    pub segments: Vec<FStringSegment>,
}
Add Expr::FString(FStringExpr) to the Expr enum.

### 2. PipelineExpr

The |> operator is a binary infix expression. It is NOT desugared in the AST —
the resolver handles threading semantics (see Task 3).

pub struct PipelineExpr {
    pub span:  Span,
    pub lhs:   Box<Expr>,
    pub rhs:   Box<Expr>,   // must resolve to a CallExpr after desugaring
}
Add Expr::Pipeline(PipelineExpr) to the Expr enum.

### 3. PropagateExpr and MustExpr

pub struct PropagateExpr {
    pub span:    Span,
    pub operand: Box<Expr>,   // must be Option<T> or Result<T, E>
}

pub struct MustExpr {
    pub span:    Span,
    pub operand: Box<Expr>,   // must be Option<T> or Result<T, E>
    pub message: Option<Box<Expr>>,   // optional Str message
}
Add Expr::Propagate(PropagateExpr) and Expr::Must(MustExpr) to the Expr enum.

### 4. Labeled loops and labeled break/continue

pub struct Label {
    pub span: Span,
    pub name: Symbol,   // the identifier after `'`, e.g. `outer` for `'outer:`
}

// Add `label` field to existing loop constructs:
pub struct WhileExpr {
    pub span:      Span,
    pub label:     Option<Label>,   // NEW — None for unlabeled
    pub condition: Box<Expr>,
    pub body:      Box<Expr>,
}

pub struct LoopExpr {
    pub span:  Span,
    pub label: Option<Label>,       // NEW
    pub body:  Box<Expr>,
}

pub struct ForExpr {
    pub span:    Span,
    pub label:   Option<Label>,     // NEW
    pub binding: Symbol,
    pub iter:    Box<Expr>,
    pub body:    Box<Expr>,
}

// Add `label` field to break and continue:
pub struct BreakExpr {
    pub span:  Span,
    pub label: Option<Label>,       // NEW — None means nearest enclosing loop
    pub value: Option<Box<Expr>>,
}

pub struct ContinueExpr {
    pub span:  Span,
    pub label: Option<Label>,       // NEW
}
### 5. Destructuring let

pub enum LetPattern {
    Ident(Symbol),                         // existing — let x = ...
    Tuple(Vec<Symbol>),                    // let (a, b, c) = ...
    Struct { name: Symbol, fields: Vec<Symbol> },  // let Point { x, y } = ...
}

pub struct LetStmt {
    pub span:    Span,
    pub pattern: LetPattern,               // CHANGED — was just Symbol
    pub mutable: bool,
    pub ty:      Option<TypeExpr>,
    pub value:   Box<Expr>,
}
LetStmt::pattern replaces the old name: Symbol field. Existing
let x = ... statements become LetPattern::Ident(x).

### 6. ImplicitNamedArg in NamedArg

pub struct NamedArg {
    pub span:     Span,
    pub name:     Symbol,
    pub value:    Box<Expr>,
    pub implicit: bool,    // NEW — true when parser inferred name from variable
}
implicit: false for all existing named args. This field is informational —
downstream stages do not branch on it.

### 7. New error variants

// Add to ParseError:
ParseError::UnterminatedFString { span: Span }
ParseError::InvalidFStringExpr  { span: Span }
ParseError::LabeledBreakOutside { label: Symbol, span: Span }
ParseError::DestructureIrrefutable { pattern: String, span: Span }

// Add to ResolveError:
ResolveError::PropagateNonFallible { ty: String, span: Span }
ResolveError::MustNonFallible      { ty: String, span: Span }
ResolveError::PipelineRhsNotCall   { span: Span }
ResolveError::LabelNotFound        { label: Symbol, span: Span }
ResolveError::DestructureArity     { expected: usize, got: usize, span: Span }

// Add to TypeError:
TypeError::MustMessageNotStr { span: Span }
All new error variants carry Span.

### Done when

- [ ] Expr::FString, Expr::Pipeline, Expr::Propagate, Expr::Must added
- [ ] Label type exists; WhileExpr, LoopExpr, ForExpr, BreakExpr, ContinueExpr have label: Option<Label>
- [ ] LetStmt::pattern: LetPattern replaces name: Symbol
- [ ] NamedArg::implicit: bool added
- [ ] All new error variants defined with Span
- [ ] Codebase compiles cleanly (no behaviour changes)

-----

## Task 2: F-String Interpolation

### What this task does

Adds f"..." as a string literal form. Interpolated expressions call .to_str()
on their value. The result type is always Str.

F-strings are **only** for Ferric string literals. Shell commands ($ ...) retain
@{expr} interpolation unchanged. Do not touch the shell lexer or parser.

### Syntax

f"literal text {expr} more text {expr2}"
- The f prefix is immediately adjacent to the opening " — no space.
- { begins an interpolated expression; } ends it.
- A literal { is escaped as {{; a literal } as }}.
- Interpolated expressions may be any valid Ferric expression, including nested
  f-strings.
- Newlines inside the string literal are allowed.

let name = "world"
let n = 42

let s = f"Hello, {name}!"              // "Hello, world!"
let t = f"The answer is {n}."          // "The answer is 42."
let u = f"Sum: {list_sum(l: [1,2,3])}" // "Sum: 6"
let v = f"{{not interpolated}}"        // "{not interpolated}"
### Lexer

Add token Token::FStringStart when the lexer sees f". The lexer then
operates in an f-string mode, emitting:

- Token::FStringLit(String) for literal segments
- Token::FStringExprStart for {
- Token::FStringExprEnd for }
- Token::FStringEnd for the closing "

The lexer handles {{ → { and }} → } escape sequences during
FStringLit emission.

No changes to the shell interpolation lexer (@{ / }).

### Parser

Parse an f-string as FStringExpr (defined in Task 1). Each {expr} segment
is parsed as a full Ferric expression using the existing expression parser.
Nested f-strings are legal.

Emit ParseError::UnterminatedFString if the closing " is not found.
Emit ParseError::InvalidFStringExpr if expression parsing inside {} fails.

### Type Checker

Expr::FString always has type Ty::Str.

Each interpolated expression e must have a type that implements ToString.
For this milestone, the following types are considered ToString-capable:
Int, Float, Bool, Str. Any other type is a TypeError.

If an expression is Ty::Str, no conversion call is emitted. Otherwise the
type checker wraps the expression in an implicit to_str coercion node.

### VM / Lowering

Evaluate each segment in order. For FStringSegmentKind::Expr, evaluate the
expression and call the appropriate conversion intrinsic (same as the existing
int_to_str, float_to_str, bool_to_str). Concatenate all segments into a
single Str value.

### Migration

No existing code changes. F-strings are purely additive.

The stdlib functions int_to_str, float_to_str, bool_to_str remain. They
are not deprecated — they are still useful as first-class function values and in
pipeline expressions.

### Done when

- [ ] Lexer emits f-string tokens; shell @{expr} unchanged
- [ ] Parser produces Expr::FString(FStringExpr)
- [ ] ParseError::UnterminatedFString and ParseError::InvalidFStringExpr emitted correctly
- [ ] Type checker assigns Ty::Str to all f-string expressions
- [ ] Type checker rejects interpolation of non-ToString types
- [ ] VM evaluates and concatenates all segments correctly
- [ ] {{ and }} escape sequences produce literal braces
- [ ] Nested f-strings work
- [ ] Test: f"Hello, {name}!" with Str, Int, Float, Bool interpolands all pass

-----

## Task 3: Pipeline Operator (|>)

### What this task does

Adds |> as a left-associative binary infix operator. lhs |> rhs_call(args...)
desugars to rhs_call(lhs, args...) — the left-hand value is prepended as the
first positional argument and then named-argument resolution runs normally.

### Syntax

let result = data
    |> list_map(f: |x| x * 2)
    |> list_filter(f: |x| x > 0)
    |> list_fold(init: 0, f: |acc, x| acc + x)
The pipeline operator has lower precedence than all existing operators. It is
left-associative. It may be broken across lines — a line ending with |> or
beginning with |> continues the expression.

### Lexer

Add Token::Pipe2 for |>. No conflict with | (logical or is ||; Ferric
has no bitwise or).

### Parser

Parse |> as a binary infix operator with the lowest precedence level (below
||). Produce Expr::Pipeline(PipelineExpr).

The right-hand side must syntactically be a call expression (possibly with no
arguments: xs |> list_reverse()). If it is not, the parser may accept it and
defer the error to the resolver.

### Resolver

Desugar each Expr::Pipeline node:

1. The LHS is evaluated and its value is the piped argument.
1. The RHS must be a CallExpr. If not, emit ResolveError::PipelineRhsNotCall.
1. Prepend a synthetic NamedArg for the first parameter of the RHS callee,
   using the piped value as the argument value and implicit: true.
1. Run normal named-argument resolution on the resulting call.

After desugaring, Expr::Pipeline nodes no longer exist. The type checker and
VM see only CallExpr.

**Example desugaring:**

// Source
data |> list_map(f: |x| x * 2)

// After resolver
list_map(l: data, f: |x| x * 2)
The first parameter of list_map is l: [T]. The piped value data is bound
to l implicitly.

### Type Checker / VM

No changes required. The resolver produces only CallExpr nodes.

### Done when

- [ ] Token::Pipe2 lexed for |>
- [ ] Parser produces Expr::Pipeline for lhs |> rhs
- [ ] Pipeline is left-associative with lowest precedence
- [ ] Resolver desugars Expr::Pipeline → CallExpr before type checking
- [ ] ResolveError::PipelineRhsNotCall emitted when RHS is not a call
- [ ] Multi-step pipelines work correctly
- [ ] Line-continuation across |> works
- [ ] Test: pipeline through list_map, list_filter, list_fold produces correct result

-----

## Task 4: ? Propagation and must Unwrap

### What this task does

Adds two postfix operators for working with Option<T> and Result<T, E>:

- expr? — propagate: return None / Err early from the enclosing function
- expr must "message" — unwrap: panic with the given message if absent/failed
- expr must (no message) — unwrap with a generic panic message

Both are only valid on Option<T> or Result<T, E>. Using either on any other
type is a ResolveError.

### Syntax

// ? propagation
fn read_port() -> Option<Int> {
    let s = env_get(name: "PORT")?
    Option::Some(str_parse_int(s: s))
}

fn connect() -> Result<Connection, Str> {
    let host = env_get(name: "HOST")?   // Option propagated as None
    let conn = db_connect(host: host)?  // Result propagated as Err
    Result::Ok(conn)
}

// must unwrap
let val = map_get(m: config, key: "required_key") must "config missing required_key"
let n   = str_parse_int(s: raw) must   // generic panic message
must is a keyword, not an operator, to keep parsing unambiguous.

### Lexer

Add Token::Question for ? (postfix context — no conflict with existing
tokens). Add Token::Must as a keyword.

### Parser

Parse expr? as Expr::Propagate(PropagateExpr { operand: expr }).

Parse expr must and expr must <str_expr> as
Expr::Must(MustExpr { operand: expr, message: Option<Box<Expr>> }).

? binds tighter than any binary operator (postfix). must binds at the same
level as unary operators.

### Resolver / Type Checker

**For Expr::Propagate:**

1. Check that the operand type is Ty::Option(_) or Ty::Result(_, _).
   If not, emit ResolveError::PropagateNonFallible.
1. Check that the enclosing function’s return type is compatible with the
   propagated type (i.e. also Option or Result). Emit a TypeError if not.
1. The expression type is the inner T from Option<T> or Result<T, E>.

**For Expr::Must:**

1. Check that the operand type is Ty::Option(_) or Ty::Result(_, _).
   If not, emit ResolveError::MustNonFallible.
1. If a message expression is present, check it has type Ty::Str.
   If not, emit TypeError::MustMessageNotStr.
1. The expression type is the inner T.

### VM

**Expr::Propagate:** evaluate the operand. If Option::None or
Result::Err(e), immediately return that value from the current call frame
(early return). Otherwise extract and produce the inner value T.

**Expr::Must:** evaluate the operand. If Option::None or Result::Err(e),
panic with the message string (or a generic "unwrap failed" if no message).
Otherwise extract and produce the inner value T.

### Done when

- [ ] Token::Question and Token::Must lexed
- [ ] Parser produces Expr::Propagate for expr?
- [ ] Parser produces Expr::Must for expr must and expr must <msg>
- [ ] Type checker resolves Propagate and Must to inner type T
- [ ] ResolveError::PropagateNonFallible and ResolveError::MustNonFallible emitted for non-fallible operands
- [ ] TypeError::MustMessageNotStr emitted for non-Str message
- [ ] VM performs early return for ? on None/Err
- [ ] VM panics with message for must on None/Err
- [ ] Test: ? propagation from nested function returns correctly
- [ ] Test: must panics with provided message
- [ ] Test: both operators on Option and Result

-----

## Task 5: Labeled Breaks and Destructuring let

### What this task does

Two independent surface syntax improvements:

1. Loop labels ('outer:) enable break 'outer and continue 'outer to
   target a specific enclosing loop.
1. Destructuring let extends the let binding to accept tuple and struct
   patterns on the left-hand side.

These share a task because both are purely syntactic with no semantic
interaction and both touch LetStmt / loop AST nodes already updated in Task 1.

-----

### Part A: Labeled Breaks

#### Syntax

'outer: for row in matrix {
    'inner: for cell in row {
        if cell == target {
            break 'outer
        }
        if cell < 0 {
            continue 'outer
        }
    }
}
Labels use the 'name sigil, immediately preceding the loop keyword. break 'label and continue 'label target the labeled loop. Unlabeled break and
continue target the nearest enclosing loop as before.

#### Lexer

Add Token::Label(Symbol) for 'identifier (a tick followed immediately by an
identifier, no space). This is distinct from any existing token. Lex it greedily
— 'outer: produces Token::Label("outer") then Token::Colon.

#### Parser

When a loop keyword (while, loop, for) is preceded by Token::Label,
record the label in the loop AST node’s label field (added in Task 1).

When parsing break or continue, check for an optional following
Token::Label and record it in BreakExpr::label / ContinueExpr::label.

#### Resolver

On entry to each labeled loop, push the label onto a label stack. On exit, pop
it. When resolving break 'label or continue 'label, walk the label stack to
find the target loop. If not found, emit ResolveError::LabelNotFound.

Validate that a break or continue without a label is inside at least one
enclosing loop. (This check already exists for unlabeled break/continue; extend
it to also validate labeled targets.)

#### VM

No structural change. The VM already implements early-exit for break and
continue via a LoopSignal mechanism (or equivalent). Extend it to carry an
optional label and unwind the call stack to the matching loop frame.

-----

### Part B: Destructuring let

#### Syntax

// Tuple destructuring
let (a, b) = some_pair()
let (x, y, z) = triple

// Struct destructuring
let Point { x, y } = get_origin()
let Config { host, port } = load_config()

// Existing ident form — unchanged
let n = 42
let mut s = "hello"
Destructuring let is **irrefutable only**. Every pattern must match all
constructors of the type. This means:

- Tuple patterns must have the exact arity of the tuple type.
- Struct patterns must name fields that exist on the struct.
- Enum patterns are **not** supported in let — use match for refutable
  patterns. Attempting an enum pattern in let is a ParseError::DestructureIrrefutable.

#### Parser

When parsing a let statement, after let (and optional mut), check for:

- ( → parse LetPattern::Tuple
- An identifier followed by { → parse LetPattern::Struct
- Otherwise → parse LetPattern::Ident as before

For LetPattern::Tuple: parse comma-separated identifiers between ( and ).
Each identifier becomes a binding.

For LetPattern::Struct: parse the struct name, then {, then comma-separated
field names (bare identifiers — no renaming syntax in this milestone), then }.

#### Resolver

For LetPattern::Tuple:

- Resolve the RHS expression.
- Check that the RHS type is a tuple with the correct arity.
  Emit ResolveError::DestructureArity if not.
- Bind each identifier to its corresponding tuple field type.

For LetPattern::Struct:

- Resolve the RHS expression.
- Check that the RHS type matches the named struct.
- For each named field, check it exists on the struct.
  Emit ResolveError::UnknownArg (reuse) for unknown field names.
- Bind each identifier to the corresponding field type.

#### VM

For LetPattern::Tuple: evaluate the RHS, extract each element by index, bind
to the corresponding local variable slots.

For LetPattern::Struct: evaluate the RHS, extract each named field by name,
bind to local variable slots.

-----

### Done when

- [ ] Token::Label(Symbol) lexed for 'identifier
- [ ] Parser records labels on while, loop, for, break, continue
- [ ] Resolver validates label targets; emits ResolveError::LabelNotFound
- [ ] VM unwinds to correct loop frame for labeled break/continue
- [ ] Test: nested loops with break 'outer exits the correct loop
- [ ] Test: continue 'outer skips to the correct loop’s next iteration
- [ ] Parser produces LetPattern::Tuple for let (a, b) = ...
- [ ] Parser produces LetPattern::Struct for let Point { x, y } = ...
- [ ] Parser emits ParseError::DestructureIrrefutable for enum patterns in let
- [ ] Resolver checks arity and field names; emits appropriate errors
- [ ] VM binds destructured values to correct local slots
- [ ] Test: tuple destructuring from a function returning (Int, Str)
- [ ] Test: struct destructuring binds correct field values
- [ ] Test: arity mismatch produces correct error

-----

## Task 6: Implicit Named Args, Method Syntax, Opaque-Only Aliases

### What this task does

Three calling convention and type system improvements:

1. **Implicit named args** — a bare variable at a call site whose name matches
   the parameter name is accepted without the explicit name: value label.
1. **Method syntax on stdlib types** — Str, [T], Map, Set, Option,
   Result and other stdlib types may be called with .method() receiver syntax.
1. **Opaque-only type aliases** — remove transparent alias semantics. All type
   aliases are opaque (newtype-style). Transparent aliases are not supported.

-----

### Part A: Implicit Named Args

#### Syntax

fn greet(name: Str, greeting: Str = "Hello") -> Str {
    f"{greeting}, {name}!"
}

let name = "world"

greet(name)                    // implicit — variable name matches param name
greet(name: name)              // explicit — still legal, just redundant
greet(name: "Ferric")          // explicit literal — label required
greet(name, greeting: "Hey")   // mixed — implicit + explicit
The rule: if a call argument is a bare identifier expression (no label), and
that identifier’s name exactly matches a parameter name of the callee, the
resolver treats it as name: identifier. If the identifier does not match any
parameter name, emit ResolveError::UnknownArg as before.

Positional-by-position calls (multiple unlabeled args where the first happens
to not match a param name) remain a ParseError::PositionalArg.

#### Parser

Update call argument parsing. An argument may now be:

1. name: expr — explicit named arg (existing)
1. ident (bare identifier, no colon) — potential implicit named arg; emit
   NamedArg { name: ident, value: Expr::Ident(ident), implicit: true }

Any argument that is not a bare identifier and has no label is still
ParseError::PositionalArg.

#### Resolver

For each NamedArg with implicit: true:

1. Check that name matches a parameter of the callee. If not, emit
   ResolveError::UnknownArg. (The user passed a variable whose name doesn’t
   match any parameter — likely a mistake.)
1. If matched, proceed as a normal named arg.

No other changes to the resolver’s arg-canonicalization logic.

-----

### Part B: Method Syntax on Stdlib Types

#### What changes

The following call forms become equivalent:

// Function form (existing — unchanged)
str_len(s: my_str)
list_map(l: xs, f: |x| x * 2)
map_get(m: config, key: "host")
option_unwrap(o: maybe_val)   // hypothetical — not a real current fn

// Method form (new)
my_str.len()
xs.map(f: |x| x * 2)
config.get(key: "host")
maybe_val.unwrap()
Method syntax receiver.method(args...) desugars in the resolver to the
corresponding namespace_method(subject: receiver, args...) call. The receiver
fills the first parameter (the “subject” parameter — s, l, m, o, etc.).

#### Method dispatch rules

The resolver maps receiver.method(args) by:

1. Determining the type of receiver.
1. Looking up method in the method table for that type (see below).
1. Desugaring to the corresponding stdlib function call with the receiver as
   the first argument.
1. If no method is found for that type, emit ResolveError::UnknownMethod.

This is not trait dispatch — it is a fixed resolver-level rewrite table. No new
trait machinery is required.

#### Method table (initial set)

The following methods are registered for each type. This list is the minimum
viable set for M10. It may be extended in future milestones.

**Str** — first param is s: Str:

|Method                |Stdlib function  |
|----------------------|-----------------|
|.len()              |str_len        |
|.contains(sub)      |str_contains   |
|.starts_with(prefix)|str_starts_with|
|.ends_with(suffix)  |str_ends_with  |
|.split(sep)         |str_split      |
|.trim()             |str_trim       |
|.to_upper()         |str_to_upper   |
|.to_lower()         |str_to_lower   |
|.replace(from, to)  |str_replace    |
|.parse_int()        |str_parse_int  |
|.parse_float()      |str_parse_float|
|.is_empty()         |str_is_empty   |
|.lines()            |str_lines      |
|.chars()            |str_chars      |
|.slice(from, to)    |str_slice      |

**[T] (List/Array)** — first param is l: [T]:

|Method           |Stdlib function |
|-----------------|----------------|
|.len()         |list_len      |
|.map(f)        |list_map      |
|.filter(f)     |list_filter   |
|.fold(init, f) |list_fold     |
|.any(f)        |list_any      |
|.all(f)        |list_all      |
|.find(f)       |list_find     |
|.contains(item)|list_contains |
|.first()       |list_first    |
|.last()        |list_last     |
|.reverse()     |list_reverse  |
|.take(n)       |list_take     |
|.drop(n)       |list_drop     |
|.append(item)  |list_append   |
|.concat(other) |list_concat   |
|.zip(other)    |list_zip      |
|.enumerate()   |list_enumerate|
|.is_empty()    |list_is_empty |

**Map<K,V>** — first param is m: Map<K,V>:

|Method                 |Stdlib function   |
|-----------------------|------------------|
|.get(key)            |map_get         |
|.get_or(key, default)|map_get_or      |
|.insert(key, val)    |map_insert      |
|.remove(key)         |map_remove      |
|.contains_key(key)   |map_contains_key|
|.keys()              |map_keys        |
|.values()            |map_values      |
|.len()               |map_len         |
|.is_empty()          |map_is_empty    |
|.merge(other)        |map_merge       |

**Option<T>** — first param is o: Option<T> (if such stdlib fns exist) or
implemented as resolver-level intrinsics:

|Method               |Behaviour                                                  |
|---------------------|-----------------------------------------------------------|
|.is_some()         |true if Some                                           |
|.is_none()         |true if None                                           |
|.unwrap()          |extract T or panic (equivalent to must with no message)|
|.unwrap_or(default)|extract T or return default                              |
|.map(f)            |apply f to inner value if Some                         |

**Result<T, E>**:

|Method               |Behaviour                       |
|---------------------|--------------------------------|
|.is_ok()           |true if Ok                  |
|.is_err()          |true if Err                 |
|.unwrap()          |extract T or panic            |
|.unwrap_or(default)|extract T or return default   |
|.map(f)            |apply f to inner value if Ok|
|.map_err(f)        |apply f to error if Err     |

#### Parser

Method call syntax expr.method(args...) is already parsed as field access +
call in most languages. The parser produces a new MethodCallExpr:

pub struct MethodCallExpr {
    pub span:     Span,
    pub receiver: Box<Expr>,
    pub method:   Symbol,
    pub args:     Vec<NamedArg>,
}
Add Expr::MethodCall(MethodCallExpr) to the Expr enum (add to Task 1’s
common types list).

The parser produces Expr::MethodCall when it sees expr.ident(. It produces
the existing field-access Expr::Field when it sees expr.ident without (.

#### Resolver

On encountering Expr::MethodCall { receiver, method, args }:

1. Resolve the receiver’s type.
1. Look up method in the method table for that type.
1. If found, desugar to the corresponding stdlib CallExpr with receiver as
   first arg, then run normal arg resolution.
1. If not found, check if the receiver’s type has a user-defined impl with
   that method name (existing trait dispatch). If so, dispatch normally.
1. If neither, emit ResolveError::UnknownMethod { method, ty, span }.

Add ResolveError::UnknownMethod { method: Symbol, ty: String, span: Span }.

Function-form calls (str_len(s: x)) remain valid. Method syntax is an
alternative, not a replacement.

-----

### Part C: Opaque-Only Type Aliases

#### What changes

type aliases are always opaque (newtype-style). There is no transparent alias
form. The as cast syntax is the only way to convert between an alias and its
underlying type.

type Url = Str
type UserId = Int

let raw: Str = "https://example.com"
let url: Url = raw as Url       // required — types are distinct
let back: Str = url as Str      // required — types are distinct

// This is a type error — Url is not Str:
fn takes_str(s: Str) -> Unit { ... }
takes_str(s: url)    // TypeError: expected Str, got Url
#### Implementation

This is primarily a type checker enforcement change:

1. When resolving a type alias, record it as **opaque** — the alias name and
   the underlying type are distinct Ty variants in the type checker.
1. Ty::Alias(name, Box<Ty>) is the type of an alias. It does not unify with
   Box<Ty> directly.
1. as casts between Ty::Alias and its underlying type are always valid and
   zero-cost.
1. Emit TypeError::AliasMismatch { expected, got, span } when an alias is
   used where the underlying type is expected or vice versa.

Add to ferric_common:

// Add to Ty enum:
Ty::Alias(Symbol, Box<Ty>)   // alias name + underlying type

// Add to TypeError:
TypeError::AliasMismatch { expected: String, got: String, span: Span }
If the current implementation already treats aliases as transparent, this is a
breaking change for any code that passes an alias where the base type is
expected without an as cast. The test suite must be audited for such cases
and updated.

-----

### Done when (Task 6)

**Implicit named args:**

- [ ] Parser emits NamedArg { implicit: true } for bare identifier args
- [ ] Resolver accepts implicit named arg when name matches a parameter
- [ ] Resolver emits ResolveError::UnknownArg when name does not match
- [ ] Test: greet(name) where let name = "world" resolves correctly

**Method syntax:**

- [ ] Expr::MethodCall added to ferric_common (retroactively to Task 1 types)
- [ ] Parser produces Expr::MethodCall for expr.method(...)
- [ ] Resolver desugars method calls to stdlib function calls for all table entries
- [ ] ResolveError::UnknownMethod emitted for unrecognized method names
- [ ] Method syntax and function syntax produce identical results for all table entries
- [ ] Test: xs.map(f: |x| x * 2).filter(f: |x| x > 0) chains correctly
- [ ] Test: my_str.split(sep: ",").len() returns correct count

**Opaque aliases:**

- [ ] Ty::Alias(Symbol, Box<Ty>) does not unify with its underlying type
- [ ] as cast between alias and underlying type is accepted and zero-cost
- [ ] TypeError::AliasMismatch emitted when alias used where base type expected
- [ ] Test suite audited and updated for new strictness
- [ ] Test: type Url = Str — Url rejected where Str expected without cast

-----

## Migration summary

|Feature            |Breaking?|Action                                          |
|-------------------|---------|------------------------------------------------|
|F-strings          |No       |Additive                                        |
||> operator      |No       |Additive                                        |
|? and must     |No       |Additive                                        |
|Labeled breaks     |No       |Additive                                        |
|Destructuring let|No       |Additive                                        |
|Implicit named args|No       |Relaxes existing restriction                    |
|Method syntax      |No       |Additive — function form still valid            |
|Opaque-only aliases|**Yes**  |Audit call sites using aliases without as cast|

The only migration burden is opaque aliases. If the current implementation
already enforces opacity, there is nothing to do. If aliases are currently
transparent, search the test suite for alias values passed to functions
expecting the underlying type, and add as casts.