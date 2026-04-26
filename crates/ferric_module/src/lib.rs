//! # Ferric Module Resolver
//!
//! Walks the import graph of a Ferric program and produces a `ModuleResult`:
//!
//! - validates each `import` path against the manifest,
//! - parses every transitively-imported file (with cycle detection),
//! - validates that named imports refer to items the target file `export`s,
//! - builds a bindings table mapping each local imported name to a synthetic
//!   `DefId` representing the source-file definition.
//!
//! The DefIds in `ModuleResult.imports[].bindings` are *synthetic* — allocated
//! from the top of the `u32` range so they never collide with the resolver's
//! `DefId`s, which start at zero. Task 4 wires these synthetic IDs into the VM.
//!
//! Public API: only `resolve_modules` is exposed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ferric_common::{
    DefId, ImportItems, ImportPath, Interner, Item, ManifestResult, ModuleError, ModuleResult,
    ParseResult, PrivateImportInfo, ResolveResult, ResolvedImport, Span, Symbol,
};
use ferric_lexer::lex;
use ferric_parser::parse_with_interner;

/// Resolves the import graph rooted at `entry_path`.
///
/// `entry_path` is the path of the source file whose `ParseResult` is `ast`.
/// `workspace_root` is the directory used to anchor `@/` paths and the
/// `.ferric/cache/` lookup; for script-mode (no manifest) it can be the parent
/// directory of `entry_path`. `interner` is mutated as new files are lexed.
///
/// The resolver is purposefully tolerant: every error is accumulated rather
/// than aborting, so a single run reports as many problems as possible.
pub fn resolve_modules(
    entry_path: &Path,
    workspace_root: &Path,
    ast: &ParseResult,
    resolve: &ResolveResult,
    manifest: &ManifestResult,
    interner: &mut Interner,
) -> ModuleResult {
    let mut ctx = ModuleCtx::new(workspace_root, manifest, interner);

    let entry_canonical = canonicalize_or_self(entry_path);
    ctx.file_asts.insert(entry_canonical.clone(), ast.clone());

    // Eagerly index entry-file exports so that any cycle that loops back to
    // it can validate its named imports against the same data.
    ctx.index_exports(&entry_canonical, ast);

    // Walk the entry file's imports recursively. The recursion detects cycles
    // and validates target paths/exports along the way.
    let mut imports_resolved: Vec<ResolvedImport> = Vec::new();
    ctx.visit_state.insert(entry_canonical.clone(), VisitState::Visiting);
    ctx.collect_imports_for_entry(&entry_canonical, ast, &mut imports_resolved);
    ctx.visit_state.insert(entry_canonical.clone(), VisitState::Done);

    // Build the entry file's exports: walk Item::Export decls and map each
    // exported item's name to a (synthetic) DefId.
    let exports = ctx.snapshot_exports(&entry_canonical);

    let _ = resolve; // currently unused; reserved for the second-pass wiring.

    let private = std::mem::take(&mut ctx.private_imports);
    ModuleResult::new(exports, imports_resolved, ctx.errors).with_private_imports(private)
}

// ============================================================================
// Internal — module-resolver context
// ============================================================================

/// Top of the synthetic DefId range. We allocate downward so the IDs never
/// collide with the resolver's normal IDs (which grow from 0).
const SYNTHETIC_DEFID_TOP: u32 = u32::MAX;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

struct ModuleCtx<'a> {
    workspace_root: PathBuf,
    manifest: &'a ManifestResult,
    interner: &'a mut Interner,

    /// Cache of parsed ASTs by canonical path.
    file_asts: HashMap<PathBuf, ParseResult>,
    /// Cache of exports per file: file → name → synthetic DefId.
    file_exports: HashMap<PathBuf, HashMap<Symbol, DefId>>,
    /// Cache of *private* (declared but not `export`ed) names per file. Used to
    /// distinguish `UnknownExport` (truly absent) from `PrivateImport`
    /// (declared but private).
    file_private_names: HashMap<PathBuf, HashSet<Symbol>>,
    /// Names accumulated for resolver wire-in: imports that targeted a private
    /// item rather than an exported one.
    private_imports: Vec<PrivateImportInfo>,
    /// DFS state for cycle detection.
    visit_state: HashMap<PathBuf, VisitState>,
    /// Order of paths on the current DFS stack — the cycle list is a slice of
    /// this, plus the closing repeated node.
    visit_stack: Vec<PathBuf>,

    /// Synthetic DefId allocator. Allocates from `u32::MAX` downward.
    next_synthetic: u32,

    errors: Vec<ModuleError>,
}

impl<'a> ModuleCtx<'a> {
    fn new(workspace_root: &Path, manifest: &'a ManifestResult, interner: &'a mut Interner) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            manifest,
            interner,
            file_asts: HashMap::new(),
            file_exports: HashMap::new(),
            file_private_names: HashMap::new(),
            private_imports: Vec::new(),
            visit_state: HashMap::new(),
            visit_stack: Vec::new(),
            next_synthetic: SYNTHETIC_DEFID_TOP,
            errors: Vec::new(),
        }
    }

    fn alloc_def_id(&mut self) -> DefId {
        let id = DefId(self.next_synthetic);
        // Saturating_sub keeps us from wrapping if a program has billions of
        // exports; that's well past any realistic use.
        self.next_synthetic = self.next_synthetic.saturating_sub(1);
        id
    }

    /// Walks the imports in `ast` (which lives at `from_file`) and:
    ///   1. validates each import path against the manifest,
    ///   2. recursively loads + indexes the target file (cycle detection),
    ///   3. validates named imports against the target's exports, and
    ///   4. appends a `ResolvedImport` for each import to `out`.
    ///
    /// `out` only collects the entry file's imports — recursive walks build
    /// the file-export cache but don't emit `ResolvedImport`s for transitive
    /// imports (those are not part of the entry's `ModuleResult.imports`).
    fn collect_imports_for_entry(
        &mut self,
        from_file: &Path,
        ast: &ParseResult,
        out: &mut Vec<ResolvedImport>,
    ) {
        for item in &ast.items {
            if let Item::Import(decl) = item {
                if let Some(resolved) = self.resolve_import_decl(from_file, decl, /*entry*/ true) {
                    out.push(resolved);
                }
            }
        }
    }

    /// Recursive variant: walk `ast`'s imports without pushing entries to a
    /// `ResolvedImport` list. Used during DFS of imported files.
    fn walk_imports_recursive(&mut self, from_file: &Path, ast: &ParseResult) {
        let imports: Vec<_> = ast
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Import(decl) => Some(decl.clone()),
                _ => None,
            })
            .collect();
        for decl in &imports {
            let _ = self.resolve_import_decl(from_file, decl, /*entry*/ false);
        }
    }

    /// Resolves one `import` declaration. Returns `Some(ResolvedImport)` only
    /// when the import is the entry file's *and* the path resolved to a file
    /// we can index. Errors are accumulated even if `None` is returned.
    fn resolve_import_decl(
        &mut self,
        from_file: &Path,
        decl: &ferric_common::ImportDecl,
        is_entry: bool,
    ) -> Option<ResolvedImport> {
        // Validate path shape against manifest, then resolve to a filesystem
        // path. If we can't get an absolute path, every branch below has
        // already pushed the appropriate error.
        let abs = self.resolve_path(from_file, &decl.path, decl.span)?;

        // Cycle detection: a target on the visit stack is a back edge.
        if matches!(self.visit_state.get(&abs), Some(VisitState::Visiting)) {
            // Build the cycle: [target ... target]. The closing edge is the
            // import we just followed.
            let cycle = self.build_cycle(&abs, &abs);
            self.errors.push(ModuleError::CircularImport {
                cycle,
                span: decl.span,
            });
            return None;
        }

        // Load target file's AST if we haven't yet.
        if !self.file_asts.contains_key(&abs) {
            match self.load_and_parse(&abs) {
                Ok(()) => {}
                Err(io_err) => {
                    // The simplest reasonable error here is UnknownExport-style
                    // — but we don't have a dedicated "file not found" variant.
                    // Re-purpose the import-path span and surface the OS error
                    // text via NoManifest's path-text channel (least surprising
                    // existing variant). A future task can introduce a richer
                    // variant.
                    self.errors.push(ModuleError::NoManifest {
                        path: format!("{}: {}", abs.display(), io_err),
                        span: decl.span,
                    });
                    return None;
                }
            }
            // Index its exports up front so cycle-target validation works.
            let target_ast = self.file_asts.get(&abs).cloned().unwrap();
            self.index_exports(&abs, &target_ast);
        }

        // Recurse into the target's own imports (DFS).
        self.visit_state.insert(abs.clone(), VisitState::Visiting);
        self.visit_stack.push(abs.clone());
        let target_ast = self.file_asts.get(&abs).cloned().unwrap();
        self.walk_imports_recursive(&abs, &target_ast);
        self.visit_stack.pop();
        self.visit_state.insert(abs.clone(), VisitState::Done);

        // Build entry-only ResolvedImport.
        if !is_entry {
            return None;
        }

        let target_exports = self.file_exports.get(&abs).cloned().unwrap_or_default();
        let path_str = self.import_path_to_string(&decl.path);
        let mut bindings: Vec<(Symbol, DefId)> = Vec::new();

        let target_private = self
            .file_private_names
            .get(&abs)
            .cloned()
            .unwrap_or_default();
        match &decl.items {
            ImportItems::Named(items) => {
                for item in items {
                    match target_exports.get(&item.name) {
                        Some(def_id) => {
                            let local = item.alias.unwrap_or(item.name);
                            bindings.push((local, *def_id));
                        }
                        None if target_private.contains(&item.name) => {
                            // The name exists in the file but isn't exported.
                            // Defer to the resolver as `PrivateImport` so the
                            // diagnostic is precise.
                            self.private_imports.push(PrivateImportInfo {
                                name: item.name,
                                path: path_str.clone(),
                                span: item.span,
                            });
                        }
                        None => {
                            self.errors.push(ModuleError::UnknownExport {
                                name: item.name,
                                path: path_str.clone(),
                                span: item.span,
                            });
                        }
                    }
                }
            }
            ImportItems::Namespace(alias) => {
                // The namespace itself binds to a single DefId; downstream
                // (Task 4) materialises a `Value::Module` from the file's
                // export table.
                let ns_def = self.alloc_def_id();
                bindings.push((*alias, ns_def));
            }
        }

        Some(ResolvedImport {
            span: decl.span,
            path: decl.path.clone(),
            bindings,
        })
    }

    /// Converts an `ImportPath` to a filesystem path. Emits the appropriate
    /// `ModuleError` and returns `None` if the path isn't usable.
    fn resolve_path(
        &mut self,
        from_file: &Path,
        path: &ImportPath,
        span: Span,
    ) -> Option<PathBuf> {
        match path {
            ImportPath::Relative(s) => {
                // `from_file` is a `*.fe` file; resolve relative to its dir.
                let parent = from_file.parent().unwrap_or_else(|| Path::new("."));
                let raw = parent.join(s);
                Some(canonicalize_or_self(&with_fe_ext(&raw)))
            }
            ImportPath::Workspace(s) => {
                // `@/foo` requires a manifest. Without one, error out.
                if self.manifest.manifest.is_none() {
                    self.errors.push(ModuleError::NoManifest {
                        path: s.clone(),
                        span,
                    });
                    return None;
                }
                let stripped = s.strip_prefix("@/").unwrap_or(s.as_str());
                let raw = self.workspace_root.join(stripped);
                Some(canonicalize_or_self(&with_fe_ext(&raw)))
            }
            ImportPath::Cache(name) => {
                let manifest = match &self.manifest.manifest {
                    Some(m) => m,
                    None => {
                        self.errors.push(ModuleError::NoManifest {
                            path: name.clone(),
                            span,
                        });
                        return None;
                    }
                };
                let version = match manifest.dependencies.get(name) {
                    Some(v) => v,
                    None => {
                        self.errors.push(ModuleError::NoManifest {
                            path: name.clone(),
                            span,
                        });
                        return None;
                    }
                };
                let cache_dir = self
                    .workspace_root
                    .join(".ferric")
                    .join("cache")
                    .join(format!("{}-{}", name, version));
                if !cache_dir.exists() {
                    self.errors.push(ModuleError::CacheMiss {
                        name: name.clone(),
                        span,
                    });
                    return None;
                }
                // Convention: cache packages have their entry at `lib.fe`.
                Some(canonicalize_or_self(&cache_dir.join("lib.fe")))
            }
        }
    }

    /// Reads + lexes + parses `path`, caching the result in `file_asts`.
    fn load_and_parse(&mut self, path: &Path) -> Result<(), std::io::Error> {
        let source = std::fs::read_to_string(path)?;
        let lex_result = lex(&source, self.interner);
        let parse_result = parse_with_interner(&lex_result, self.interner);
        self.file_asts.insert(path.to_path_buf(), parse_result);
        Ok(())
    }

    /// Walks `ast` and indexes per-file naming info:
    ///   - `file_exports[file]` — name → synthetic DefId for every
    ///     `Item::Export(item)`.
    ///   - `file_private_names[file]` — set of names for *unexported* top-level
    ///     items. The named-import classifier uses this to distinguish
    ///     `UnknownExport` (truly absent) from `PrivateImport` (present but
    ///     private).
    fn index_exports(&mut self, file: &Path, ast: &ParseResult) {
        if self.file_exports.contains_key(file) {
            return;
        }
        let mut exports: HashMap<Symbol, DefId> = HashMap::new();
        let mut private: HashSet<Symbol> = HashSet::new();
        for item in &ast.items {
            match item {
                Item::Export(decl) => {
                    if let Some(name) = exported_name(&decl.item) {
                        let def_id = self.alloc_def_id();
                        exports.insert(name, def_id);
                    }
                }
                other => {
                    if let Some(name) = exported_name(other) {
                        private.insert(name);
                    }
                }
            }
        }
        self.file_exports.insert(file.to_path_buf(), exports);
        self.file_private_names.insert(file.to_path_buf(), private);
    }

    /// Returns a snapshot of `file`'s export table or an empty map.
    fn snapshot_exports(&self, file: &Path) -> HashMap<Symbol, DefId> {
        self.file_exports.get(file).cloned().unwrap_or_default()
    }

    /// Renders an `ImportPath` back to its source string for error messages.
    fn import_path_to_string(&self, path: &ImportPath) -> String {
        match path {
            ImportPath::Relative(s) | ImportPath::Workspace(s) | ImportPath::Cache(s) => s.clone(),
        }
    }

    /// Builds the cycle list: the slice of `visit_stack` from `target` onward,
    /// plus `closing` to close the loop. File names are reduced to their
    /// `Display` form for diagnostics.
    fn build_cycle(&self, target: &Path, closing: &Path) -> Vec<String> {
        let start_idx = self
            .visit_stack
            .iter()
            .position(|p| p == target)
            .unwrap_or(0);
        let mut cycle: Vec<String> = self.visit_stack[start_idx..]
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        cycle.push(closing.display().to_string());
        cycle
    }
}

// ============================================================================
// Internal — small helpers
// ============================================================================

/// Returns the symbol naming the exported item, or `None` for items that
/// aren't named (script blocks etc., which cannot legally be exported).
fn exported_name(item: &Item) -> Option<Symbol> {
    match item {
        Item::FnDef { name, .. }
        | Item::StructDef { name, .. }
        | Item::EnumDef { name, .. }
        | Item::TraitDef { name, .. } => Some(*name),
        Item::TypeAlias(decl) => Some(decl.name),
        Item::ImplBlock { .. } | Item::Script { .. } | Item::Import(_) | Item::Export(_) => None,
    }
}

/// Adds a `.fe` extension if the path doesn't already have one. Pure helper —
/// does not touch the filesystem.
fn with_fe_ext(p: &Path) -> PathBuf {
    if p.extension().is_some() {
        p.to_path_buf()
    } else {
        let mut out = p.to_path_buf();
        out.set_extension("fe");
        out
    }
}

/// Canonicalises if possible; falls back to the input path otherwise so we
/// always have *some* PathBuf to use as a map key. Two paths that both
/// canonicalise differ only by symlink resolution, which is acceptable.
fn canonicalize_or_self(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

// Compile-time assertion: the resolver only depends on send-safe types so it
// can run inside an async runtime in a future milestone.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<ModuleResult>();
    assert_send::<HashSet<PathBuf>>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Interner, Manifest};
    use std::fs;

    /// Tiny private tempdir helper. Nanos alone collide under parallel tests
    /// on coarse-resolution clocks; pair them with an atomic counter so each
    /// call always gets a unique directory.
    fn tempdir() -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        p.push(format!("ferric-module-test-{}-{}-{}", std::process::id(), nonce, n));
        std::fs::create_dir_all(&p).unwrap();
        TempDir { path: p }
    }
    struct TempDir {
        path: PathBuf,
    }
    impl TempDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    fn lex_parse(interner: &mut Interner, src: &str) -> ParseResult {
        let lex_result = lex(src, interner);
        parse_with_interner(&lex_result, interner)
    }

    fn empty_resolve() -> ResolveResult {
        ResolveResult::new(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            Vec::new(),
        )
    }

    #[test]
    fn relative_import_resolves_named_export() {
        let dir = tempdir();
        let entry_path = write_file(
            dir.path(),
            "main.fe",
            r#"import { greet } from "./util"
greet()
"#,
        );
        write_file(
            dir.path(),
            "util.fe",
            "export fn greet() { }\n",
        );

        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let manifest = ManifestResult::new(None, Vec::new());
        let resolve = empty_resolve();

        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &resolve,
            &manifest,
            &mut interner,
        );

        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.imports.len(), 1);
        let r = &result.imports[0];
        assert_eq!(r.bindings.len(), 1);
        assert_eq!(interner.resolve(r.bindings[0].0), "greet");
    }

    #[test]
    fn unknown_export_is_an_error() {
        let dir = tempdir();
        let entry_path = write_file(
            dir.path(),
            "main.fe",
            r#"import { missing } from "./util"
"#,
        );
        write_file(dir.path(), "util.fe", "export fn other() { }\n");

        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let manifest = ManifestResult::new(None, Vec::new());
        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &empty_resolve(),
            &manifest,
            &mut interner,
        );
        assert!(result.errors.iter().any(|e| matches!(e, ModuleError::UnknownExport { .. })));
    }

    #[test]
    fn workspace_path_without_manifest_errors() {
        let dir = tempdir();
        let entry_path = write_file(
            dir.path(),
            "main.fe",
            r#"import { Config } from "@/config"
"#,
        );
        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let manifest = ManifestResult::new(None, Vec::new());
        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &empty_resolve(),
            &manifest,
            &mut interner,
        );
        assert!(result.errors.iter().any(|e| matches!(e, ModuleError::NoManifest { .. })));
    }

    #[test]
    fn cache_path_without_dep_in_manifest_errors() {
        let dir = tempdir();
        let entry_path = write_file(
            dir.path(),
            "main.fe",
            r#"import { Foo } from "ferric-http"
"#,
        );
        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let manifest = ManifestResult::new(
            Some(Manifest {
                name: "demo".into(),
                version: "0.1.0".into(),
                submodules: vec![],
                dependencies: HashMap::new(),
            }),
            Vec::new(),
        );
        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &empty_resolve(),
            &manifest,
            &mut interner,
        );
        assert!(result.errors.iter().any(|e| matches!(e, ModuleError::NoManifest { .. })));
    }

    #[test]
    fn cache_miss_when_dep_listed_but_not_present() {
        let dir = tempdir();
        let entry_path = write_file(
            dir.path(),
            "main.fe",
            r#"import { Foo } from "ferric-http"
"#,
        );
        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let mut deps: HashMap<String, String> = HashMap::new();
        deps.insert("ferric-http".into(), "1.2.0".into());
        let manifest = ManifestResult::new(
            Some(Manifest {
                name: "demo".into(),
                version: "0.1.0".into(),
                submodules: vec![],
                dependencies: deps,
            }),
            Vec::new(),
        );
        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &empty_resolve(),
            &manifest,
            &mut interner,
        );
        assert!(result.errors.iter().any(|e| matches!(e, ModuleError::CacheMiss { .. })));
    }

    #[test]
    fn circular_import_is_detected() {
        let dir = tempdir();
        // a.fe → b.fe → a.fe
        let entry_path = write_file(
            dir.path(),
            "a.fe",
            r#"import { foo } from "./b"
export fn bar() { }
"#,
        );
        write_file(
            dir.path(),
            "b.fe",
            r#"import { bar } from "./a"
export fn foo() { }
"#,
        );
        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let manifest = ManifestResult::new(None, Vec::new());
        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &empty_resolve(),
            &manifest,
            &mut interner,
        );
        assert!(result.errors.iter().any(|e| matches!(e, ModuleError::CircularImport { .. })));
    }

    #[test]
    fn namespace_import_binds_alias() {
        let dir = tempdir();
        let entry_path = write_file(
            dir.path(),
            "main.fe",
            r#"import * as util from "./util"
"#,
        );
        write_file(dir.path(), "util.fe", "export fn helper() { }\n");
        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let manifest = ManifestResult::new(None, Vec::new());
        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &empty_resolve(),
            &manifest,
            &mut interner,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.imports.len(), 1);
        assert_eq!(result.imports[0].bindings.len(), 1);
        assert_eq!(interner.resolve(result.imports[0].bindings[0].0), "util");
    }

    #[test]
    fn entry_file_exports_are_collected() {
        let dir = tempdir();
        let entry_path = write_file(
            dir.path(),
            "lib.fe",
            r#"export fn pub_fn() { }
export struct Pub { x: Int }
fn private_fn() { }
"#,
        );
        let mut interner = Interner::new();
        let entry_src = fs::read_to_string(&entry_path).unwrap();
        let entry_ast = lex_parse(&mut interner, &entry_src);
        let manifest = ManifestResult::new(None, Vec::new());
        let result = resolve_modules(
            &entry_path,
            dir.path(),
            &entry_ast,
            &empty_resolve(),
            &manifest,
            &mut interner,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let exported_names: Vec<&str> = result
            .exports
            .keys()
            .map(|s| interner.resolve(*s))
            .collect();
        assert!(exported_names.contains(&"pub_fn"));
        assert!(exported_names.contains(&"Pub"));
        assert!(!exported_names.contains(&"private_fn"));
    }
}
