# M7 — Task 2: Lexer + Parser

> **Prerequisite:** Task 1 must be complete and the codebase compiling before
> starting this task. Tasks 2 and 3 are independent of each other and may run
> concurrently if two agents are available.

---

## What this task does

Adds lexer tokens and parser rules for `import` declarations, `export` modifiers,
`type` alias definitions, and `as` cast expressions. After this task, the parser
produces correct AST nodes for all module-system syntax — but nothing is resolved
or type-checked yet.

---

## Lexer

### New reserved keyword tokens

```rust
// Add to token enum:
Import,   // `import`
Export,   // `export`
From,     // `from`  — reserved keyword, not contextual, to avoid ambiguity
Type,     // `type`  — if not already present
```

`As` should already exist from M2.5 or earlier. If not, add it here.

`From` is reserved rather than contextual because it appears in a fixed position
in every import declaration and reserving it costs almost nothing — `from` is
rarely a useful variable name.

### No new lexer error variants

All lexer-level errors for this feature are path string errors handled in the
parser. The lexer emits string literal tokens normally.

---

## Parser

### Import declarations

Import declarations must appear before any non-import item at the top level of a
file. The first non-import item closes the import section. Any `import` encountered
after that point emits `ParseError::LateImport` and is skipped (accumulate, don't abort).

#### Grammar

```
import_decl ::= "import" import_items "from" string_lit

import_items ::=
    "{" named_import ("," named_import)* ","? "}"   // named
  | "*" "as" ident                                   // namespace

named_import ::= ident ("as" ident)?
```

#### Path validation

After parsing the `string_lit`, classify the path:

- Starts with `"./"` or `"../"` → `ImportPath::Relative`
- Starts with `"@/"` → `ImportPath::Workspace`
- Contains no `/` prefix and is a valid identifier string → `ImportPath::Cache`
- Anything else → `ParseError::InvalidImportPath` (accumulate, continue)

#### Default import detection

If the parser sees `import Ident from` (capitalised or lowercase, no braces, no `*`),
emit `ParseError::DefaultImport` with the span of the identifier and skip the declaration.
This is a clear error message for users coming from JavaScript/TypeScript.

#### Examples

```rust
// LEGAL
import { connect, disconnect } from "./db"
import { connect as dbConnect } from "./db"
import * as db from "./db"
import { HttpClient } from "ferric-http"
import { Config } from "@/config"

// ILLEGAL — ParseError::DefaultImport
import db from "./db"

// ILLEGAL — ParseError::LateImport (import after a fn definition)
fn foo() { }
import { bar } from "./bar"

// ILLEGAL — ParseError::InvalidImportPath
import { X } from "not/a/valid/cache/name"
```

### Export declarations

`export` is a prefix keyword on any top-level item. It wraps the parsed item in
`ExportDecl`. The inner item is parsed exactly as it would be without `export` —
no change to inner item parsing rules.

Valid exportable items: `fn`, `struct`, `enum`, `type` aliases. Any other item
after `export` (e.g. `let` at top level, or `export` inside a function body)
emits `ParseError::InvalidExportPosition`.

```rust
// LEGAL
export fn connect(host: Str, port: Int) -> Connection { ... }
export struct Config { host: Str, port: Int }
export type Url = Str

// ILLEGAL — ParseError::InvalidExportPosition
fn foo() {
    export let x = 1
}
```

### Type alias definitions

```
type_alias ::= "type" ident ("<" ident ("," ident)* ">")? "=" type_expr
```

Produces `TypeAliasItem`. The `opaque` field is always `true` in M7.

```rust
// Non-generic
type Url = Str
type UserId = Int

// Generic
type Result<T> = StdResult<T, AppError>
type Pair<A, B> = Tuple<A, B>
```

### Cast expressions

`as` is a binary operator with **lower precedence than field access, higher
precedence than all binary arithmetic and comparison operators**. This means:

```rust
a + b as Url        // parses as: a + (b as Url)
obj.field as Url    // parses as: (obj.field) as Url
a as Url == b       // parses as: (a as Url) == b  — type checker will catch if wrong
```

Add `as` to the precedence table between field access and binary operators.
Produces `CastExpr { expr, target }`.

```rust
// LEGAL
let u: Url = "example.com" as Url
let s: Str = u as Str
let n = some_expr as Int

// LEGAL — cast on a complex expression
let u = get_raw_url() as Url
```

`as` does not chain without parentheses — `x as A as B` is a parse error
(`ParseError::ChainedCast`). This prevents confusion and can be relaxed later if needed.

---

## Error summary

All new parse errors emitted by this task (declared in Task 1, all carry `Span` — Rule 5):

```rust
ParseError::LateImport            { span: Span }
ParseError::DefaultImport         { span: Span }
ParseError::InvalidImportPath     { span: Span }
ParseError::InvalidExportPosition { span: Span }
ParseError::ChainedCast           { span: Span }
```

---

## Diagnostics

Add rendering arms for all five new `ParseError` variants. Error messages should
be clear about what the user should do instead:

```
error: default imports are not supported in Ferric
  --> src/main.fe:1:8
   |
 1 | import db from "./db"
   |        ^^ use named imports: `import { connect } from "./db"`

error: import declarations must appear before other items
  --> src/main.fe:5:1
   |
 5 | import { bar } from "./bar"
   |         ^^^^^^^^^^^^^^^^^^^ move this import to the top of the file

error: cannot chain cast expressions
  --> src/main.fe:8:14
   |
 8 |     let x = y as Foo as Bar
   |               ^^^^^^^^^^^^^^ wrap in parentheses: `(y as Foo) as Bar`
```

---

## Done when

- [ ] `Import`, `Export`, `From`, `Type` are reserved keyword tokens in the lexer
- [ ] Named imports parse correctly: `import { X, Y as Z } from "path"`
- [ ] Namespace imports parse correctly: `import * as ns from "path"`
- [ ] All three path shapes classify correctly: `./`, `@/`, bare name
- [ ] `ParseError::InvalidImportPath` fires for malformed paths
- [ ] `ParseError::DefaultImport` fires for `import X from "..."` syntax
- [ ] `ParseError::LateImport` fires for imports after non-import items
- [ ] `export fn` / `export struct` / `export enum` / `export type` parse into `ExportDecl`
- [ ] `ParseError::InvalidExportPosition` fires for `export` inside a function body
- [ ] `type Url = Str` parses into `TypeAliasItem` with `opaque: true`
- [ ] Generic type aliases (`type Result<T> = ...`) parse correctly
- [ ] `expr as TypeExpr` parses into `CastExpr` with correct precedence
- [ ] `ParseError::ChainedCast` fires for `x as A as B`
- [ ] All five new `ParseError` variants render correctly through the diagnostics renderer
- [ ] `ferric --dump-ast` output includes `ImportDecl`, `ExportDecl`, `TypeAliasItem`, and `CastExpr` nodes
- [ ] All new error types carry `Span` (Rule 5)
- [ ] All M1–M6 tests still pass
