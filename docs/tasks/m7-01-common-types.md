# M7 — Task 1: Common Types

> **Do this task first.** It adds all new types to `ferric_common` that every
> other M7 task depends on. Tasks 2, 3, and 4 may not begin until this task is
> complete and the codebase compiles cleanly.

---

## What this task does

Adds the AST nodes, stage output types, and error variants that the module system
requires. No behaviour changes — this task only extends data structures. Nothing
is wired up yet; that happens in Tasks 2–4.

---

## ferric_common — additions required

### 1. `ImportDecl`

```rust
pub struct ImportDecl {
    pub span:  Span,
    pub path:  ImportPath,
    pub items: ImportItems,
}

pub enum ImportPath {
    Relative(String),    // "./db", "../util"
    Workspace(String),   // "@/config"
    Cache(String),       // "ferric-http"
}

pub enum ImportItems {
    Named(Vec<ImportItem>),   // { connect, disconnect as d }
    Namespace(Symbol),        // * as db
}

pub struct ImportItem {
    pub span:  Span,
    pub name:  Symbol,
    pub alias: Option<Symbol>,
}
```

### 2. `ExportDecl`

`export` is a modifier on an existing top-level item, not a standalone statement.
The parser wraps the inner item in `ExportDecl`.

```rust
pub struct ExportDecl {
    pub span: Span,
    pub item: Box<Item>,   // the decorated fn, struct, enum, or type alias
}
```

### 3. `TypeAliasItem`

```rust
pub struct TypeAliasItem {
    pub span:   Span,
    pub name:   Symbol,
    pub params: Vec<Symbol>,      // generic params — empty for non-generic aliases
    pub ty:     TypeExpr,
    pub opaque: bool,             // always true in M7; reserved for future transparent aliases
}
```

### 4. `CastExpr`

```rust
pub struct CastExpr {
    pub span:   Span,
    pub expr:   Box<Expr>,
    pub target: TypeExpr,
}
```

Add `Expr::Cast(CastExpr)` to the `Expr` enum alongside existing variants.

### 5. `ModuleResult`

New stage output type returned by `ferric_module`. Follows the same shape as all
other stage output types.

```rust
pub struct ModuleResult {
    pub exports:  HashMap<Symbol, DefId>,    // name → DefId for all exported items
    pub imports:  Vec<ResolvedImport>,
    pub errors:   Vec<ModuleError>,
}

pub struct ResolvedImport {
    pub span:     Span,
    pub path:     ImportPath,
    pub bindings: Vec<(Symbol, DefId)>,      // local name → source DefId
}

pub enum ModuleError {
    CircularImport  { cycle: Vec<String>, span: Span },
    UnknownExport   { name: Symbol, path: String, span: Span },
    NoManifest      { path: String, span: Span },
    CacheMiss       { name: String, span: Span },
    DefaultImport   { span: Span },
}
```

All `ModuleError` variants carry `Span` (Rule 5).

### 6. `ManifestResult`

New stage output type returned by `ferric_manifest`.

```rust
pub struct ManifestResult {
    pub manifest: Option<Manifest>,    // None in script mode
    pub errors:   Vec<ManifestError>,
}

pub struct Manifest {
    pub name:         String,
    pub version:      String,
    pub submodules:   Vec<String>,
    pub dependencies: HashMap<String, String>,   // name → version constraint
}

pub enum ManifestError {
    ParseError          { message: String, span: Span },
    ConflictingManifest { path: String, span: Span },
}
```

All `ManifestError` variants carry `Span` (Rule 5).

### 7. New `Ty` variant

```rust
// Add to the Ty enum in ferric_common:
Ty::Opaque { def_id: DefId, inner: Box<Ty> }
```

### 8. New error variants

```rust
// Add to ParseError:
ParseError::LateImport            { span: Span }   // import after non-import item
ParseError::DefaultImport         { span: Span }   // import X from "..."
ParseError::InvalidImportPath     { span: Span }   // malformed path string
ParseError::InvalidExportPosition { span: Span }   // export inside a function body
ParseError::ChainedCast           { span: Span }   // x as A as B

// Add to ResolveError:
ResolveError::PrivateImport { name: Symbol, path: String, span: Span }

// Add to TypeError:
TypeError::OpaqueTypeMismatch { expected: Ty, found: Ty, span: Span }
TypeError::InvalidCast        { from: Ty, to: Ty, span: Span }
```

All variants carry `Span` (Rule 5).

### 9. Add `ImportDecl`, `ExportDecl`, and `TypeAliasItem` to `Item`

```rust
// Add to the Item enum:
Item::Import(ImportDecl)
Item::Export(ExportDecl)
Item::TypeAlias(TypeAliasItem)
```

### 10. Serialisation and async gate

All new types must:
- Derive `Serialize + Deserialize` (consistent with Task 4 of M2.5)
- Derive `Debug + Clone + PartialEq`
- Be `Send + Sync` — add them to the compile-time assertion in `ferric_common/src/lib.rs`:

```rust
fn _assert_send_sync() {
    fn check<T: Send + Sync>() {}
    // ... existing checks ...
    check::<ImportDecl>();
    check::<ExportDecl>();
    check::<TypeAliasItem>();
    check::<CastExpr>();
    check::<ModuleResult>();
    check::<ManifestResult>();
}
```

---

## Done when

- [ ] `ImportDecl`, `ExportDecl`, `TypeAliasItem`, `CastExpr` exist in `ferric_common`
- [ ] `Expr::Cast` variant exists
- [ ] `Item::Import`, `Item::Export`, `Item::TypeAlias` variants exist
- [ ] `ModuleResult` and `ManifestResult` exist with all fields and error variants
- [ ] `Ty::Opaque` variant exists
- [ ] All new `ParseError`, `ResolveError`, and `TypeError` variants exist
- [ ] All new types derive `Serialize + Deserialize + Debug + Clone + PartialEq`
- [ ] All new types are covered by the `Send + Sync` compile-time assertion
- [ ] Codebase compiles cleanly with no warnings
- [ ] All M1–M6 tests still pass (no behaviour change — data structures only)
