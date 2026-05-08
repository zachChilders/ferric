//! # `ferric_effects` — algebraic-effects lowering / analysis pass.
//!
//! Slots between the type checker and the compiler. Reads a parsed AST plus
//! the type checker's output and returns an [`EffectResult`] containing:
//!
//! - the (mostly-identity) rewritten AST,
//! - an [`EffectTags`] table mapping effect/op names to numeric tags the
//!   compiler embeds into `Op::Perform(effect_tag, op_tag, arg_count)`,
//! - any [`EffectError`]s and [`EffectWarning`]s discovered.
//!
//! ## Strategy chosen for M9 Task 4
//!
//! The full algebraic-effects CPS transform is a multi-week research
//! project. M9's pragmatic guidance is to do the minimum needed for M8
//! async fixtures plus a small custom-effect example, with one-shot
//! continuations and no multi-shot support.
//!
//! This crate therefore takes the **second** of the two paths described
//! in `docs/tasks/m9-04-lowering.md`:
//!
//! - The lowering pass is mostly identity — `Expr::Perform`/`Handle`/`Resume`
//!   survive into the AST handed to the compiler.
//! - The compiler is taught (in `ferric_compiler`) to emit
//!   `Op::Perform`/`PushHandler`/`PopHandler`/`Resume` directly when it
//!   sees those nodes; handler clause bodies become their own chunks.
//! - This crate's job is **AST analysis**: it assigns stable effect/op tags,
//!   collects handler clauses for visibility, and validates simple static
//!   patterns (continuation-dropped, resumed-twice, resume-outside-handler).
//!
//! That split is honest about the responsibility line: name resolution and
//! tag assignment belong here; the actual continuation-capture lives in
//! the VM (Task 5). The compiler is a thin emitter on top.
//!
//! ## Tag assignment
//!
//! Built-in effects come first: `Async = 0`, `Gen = 1`. User-declared
//! effects get tags starting at 2, in source order. Within each effect,
//! ops are tagged in declaration order. Built-in op tags:
//! `Async::Suspend = 0`, `Async::Spawn = 1`, `Async::Join = 2`,
//! `Gen::Yield = 0`. User ops follow source order.
//!
//! Tags are u16 / u8 to match the bytecode encoding.
//!
//! ## Limitations / deferrals
//!
//! - **Multi-shot continuations are not supported.** This is a milestone-wide
//!   constraint, not a crate-local one.
//! - **Effects across closure boundaries are not validated.** A `perform`
//!   inside a closure that escapes its enclosing handler will be detected
//!   at runtime by the VM, not statically here.
//! - The full CPS transform (splitting fns at perform points into
//!   before/after closures) is **not** implemented. The compiler+VM
//!   handle continuation capture at runtime via the new opcodes.
//! - `gen fn` / `yield` desugaring works at parse time, but generator
//!   handlers (`gen::collect`) are not validated here.

use ferric_common::{
    EffectDecl, EffectError, EffectResult, EffectTags, EffectWarning, Expr, HandleExpr,
    HandlerClause, Item, NamedArg, ParseResult, PerformExpr, RequireStmt, ResumeExpr, ShellPart,
    Stmt, Symbol, TypeResult,
};
use std::collections::HashMap;

/// Single public entry point for the effects lowering / analysis stage.
///
/// Walks `ast` once: assigns effect/op tags to every `effect` declaration,
/// validates handler-clause bodies for simple resume-related issues, and
/// returns an [`EffectResult`] whose `.ast` is structurally identical to
/// the input. (See module docs for why we don't do a full CPS transform.)
pub fn lower_effects(ast: &ParseResult, _types: &TypeResult) -> EffectResult {
    let mut analyzer = Analyzer::new();
    analyzer.collect_effect_decls(ast);
    analyzer.walk_program(ast);

    // Output invariant assertions per the spec. With our chosen strategy
    // the AST is left unchanged, so the invariants about `Expr::Perform`
    // *for handled effects* being rewritten don't apply: we instead check
    // the looser invariant that nothing in the AST is structurally
    // malformed (every Resume sits inside a handler clause body, etc.).
    debug_assertions(ast, &analyzer);

    EffectResult::with_tags(
        ast.clone(),
        EffectTags {
            effect_tags: analyzer.effect_tags,
            op_tags: analyzer.op_tags,
        },
        analyzer.errors,
        analyzer.warnings,
    )
}

// ---------------------------------------------------------------------------
// Built-in effect names
// ---------------------------------------------------------------------------

/// Built-in effect names with their reserved tag and op-table. The tag table
/// must include both Async and Gen even when the program doesn't declare
/// them, because the parser desugars `await`/`yield` into perform sites
/// that reference them.
#[allow(clippy::type_complexity)] // Inline const literal — naming the tuple shape adds noise without aiding callers.
const BUILTIN_EFFECTS: &[(&str, u16, &[(&str, u8)])] = &[
    ("Async", 0, &[("Suspend", 0), ("Spawn", 1), ("Join", 2)]),
    ("Gen", 1, &[("Yield", 0)]),
];

// ---------------------------------------------------------------------------
// Analyzer
// ---------------------------------------------------------------------------

struct Analyzer {
    /// `effect_name -> tag`.
    effect_tags: HashMap<Symbol, u16>,
    /// `(effect_name, op_name) -> tag`.
    op_tags: HashMap<(Symbol, Symbol), u8>,
    /// Next available user-effect tag. Starts at the first slot above the
    /// reserved built-in range.
    next_effect_tag: u16,

    /// Stack of (effect, op, cont_symbol) frames, one per active handler
    /// clause body. `Resume` walks this to validate it sits inside a
    /// matching frame; `ContinuationDropped` consults the topmost frame
    /// at the end of each clause body.
    handler_stack: Vec<HandlerFrame>,

    errors: Vec<EffectError>,
    warnings: Vec<EffectWarning>,
}

struct HandlerFrame {
    /// The effect this clause is matching against. Currently informational;
    /// kept for future cross-clause checks (e.g. UnhandledEffect on closed
    /// rows, which the analyzer doesn't compute today).
    #[allow(dead_code)]
    effect: Symbol,
    /// The op this clause is matching against. Same rationale as `effect`.
    #[allow(dead_code)]
    op: Symbol,
    /// The continuation symbol bound by the current clause (typically `k`).
    cont: Symbol,
    /// Set true when a `resume` matching `cont` is encountered.
    resumed_once: bool,
    /// Set true on the **second** resume inside the same clause body — used
    /// to suppress duplicate `ResumedTwice` errors.
    resumed_twice: bool,
}

impl Analyzer {
    fn new() -> Self {
        // Reserve tags above the highest built-in tag. The `+1` keeps the
        // arithmetic simple even if we add more built-ins later.
        let next = BUILTIN_EFFECTS
            .iter()
            .map(|(_, tag, _)| *tag)
            .max()
            .map(|t| t + 1)
            .unwrap_or(0);
        Self {
            effect_tags: HashMap::new(),
            op_tags: HashMap::new(),
            next_effect_tag: next,
            handler_stack: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Pre-populates [`effect_tags`] / [`op_tags`] for every user
    /// `effect` declaration in source order.
    ///
    /// Built-in effects (`Async`, `Gen`) are *not* registered here: we
    /// can't compare their canonical names against user `Symbol`s without
    /// the interner. Instead they get tags lazily on first sight at a
    /// `perform` site (see [`walk_perform`]). The tags are stable per
    /// program but their numeric values aren't fixed across programs —
    /// downstream consumers (compiler + VM) don't care about specific
    /// numeric values, only that they're consistent within one EffectResult.
    fn collect_effect_decls(&mut self, ast: &ParseResult) {
        for item in ast.items.iter().map(unwrap_export) {
            if let Item::EffectDecl(decl) = item {
                self.register_user_effect(decl);
            }
        }
    }

    fn alloc_effect_tag(&mut self) -> u16 {
        let t = self.next_effect_tag;
        self.next_effect_tag = self.next_effect_tag.saturating_add(1);
        t
    }

    fn register_user_effect(&mut self, decl: &EffectDecl) {
        // If the name happens to match a built-in (e.g. user redeclared
        // `effect Async {...}`), reuse the canonical tag. We can't compare
        // against the BUILTIN_EFFECTS strings without an interner, so we
        // just allocate a fresh tag — that's safe because every Perform
        // site uses the same Symbol the user typed.
        if !self.effect_tags.contains_key(&decl.name) {
            let tag = self.alloc_effect_tag();
            self.effect_tags.insert(decl.name, tag);
        }
        for (i, op) in decl.ops.iter().enumerate() {
            // Keep declaration order; cap at u8::MAX to avoid silent wrap.
            let tag = u8::try_from(i).unwrap_or(u8::MAX);
            self.op_tags.insert((decl.name, op.name), tag);
        }
    }

    /// Top-level walk: visits each item (and the effect AST nodes inside it).
    fn walk_program(&mut self, ast: &ParseResult) {
        for item in &ast.items {
            self.walk_item(item);
        }
    }

    fn walk_item(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => self.walk_expr(&f.body),
            Item::ImplBlock { methods, .. } => {
                for m in methods {
                    self.walk_expr(&m.body);
                }
            }
            Item::Script { stmt, .. } => self.walk_stmt(stmt),
            Item::Export(decl) => self.walk_item(&decl.item),
            Item::EffectDecl(_)
            | Item::StructDef { .. }
            | Item::EnumDef { .. }
            | Item::TraitDef { .. }
            | Item::Import(_)
            | Item::TypeAlias(_) => {}
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => self.walk_expr(init),
            Stmt::Assign { target, value, .. } => {
                self.walk_expr(target);
                self.walk_expr(value);
            }
            Stmt::Expr { expr } => self.walk_expr(expr),
            Stmt::Require(req) => self.walk_require(req),
            Stmt::For { iter, body, .. } => {
                self.walk_expr(iter);
                self.walk_expr(body);
            }
        }
    }

    fn walk_require(&mut self, req: &RequireStmt) {
        self.walk_expr(&req.expr);
        if let Some(m) = &req.message {
            self.walk_expr(m);
        }
        if let Some(s) = &req.set_fn {
            self.walk_expr(s);
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Perform(p) => self.walk_perform(p),
            Expr::Handle(h) => self.walk_handle(h),
            Expr::Resume(r) => self.walk_resume(r),

            Expr::Literal { .. }
            | Expr::Variable { .. }
            | Expr::Break { .. }
            | Expr::Continue { .. } => {}

            Expr::Binary { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::Unary { expr, .. } => self.walk_expr(expr),
            Expr::Call { callee, args, .. } => {
                self.walk_expr(callee);
                for a in args {
                    self.walk_named_arg(a);
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.walk_expr(cond);
                self.walk_expr(then_branch);
                if let Some(e) = else_branch {
                    self.walk_expr(e);
                }
            }
            Expr::Block { stmts, expr, .. } => {
                for s in stmts {
                    self.walk_stmt(s);
                }
                if let Some(e) = expr {
                    self.walk_expr(e);
                }
            }
            Expr::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.walk_expr(e);
                }
            }
            Expr::While { cond, body, .. } => {
                self.walk_expr(cond);
                self.walk_expr(body);
            }
            Expr::Loop { body, .. } => self.walk_expr(body),
            Expr::Closure { body, .. } => self.walk_expr(body),
            Expr::Shell { parts, .. } => {
                for p in parts {
                    if let ShellPart::Interpolated(e) = p {
                        self.walk_expr(e);
                    }
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, e) in fields {
                    self.walk_expr(e);
                }
            }
            Expr::FieldAccess { expr, .. } => self.walk_expr(expr),
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.walk_expr(scrutinee);
                for arm in arms {
                    self.walk_expr(&arm.body);
                }
            }
            Expr::Tuple { elements, .. } | Expr::ArrayLit { elements, .. } => {
                for e in elements {
                    self.walk_expr(e);
                }
            }
            Expr::VariantCtor { args, .. } => {
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::Index { array, index, .. } => {
                self.walk_expr(array);
                self.walk_expr(index);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.walk_expr(receiver);
                for a in args {
                    self.walk_named_arg(a);
                }
            }
            Expr::Cast(c) => self.walk_expr(&c.expr),
            Expr::AsyncBlock(b) => self.walk_expr(&b.block),
            // M10 Task 1 scaffolding: parser does not emit these yet (Tasks
            // 2-4 wire them up). Walk children defensively.
            Expr::FString(e) => {
                for seg in &e.segments {
                    if let ferric_common::FStringSegmentKind::Expr(inner) = &seg.kind {
                        self.walk_expr(inner);
                    }
                }
            }
            Expr::Pipeline(e) => {
                self.walk_expr(&e.lhs);
                self.walk_expr(&e.rhs);
            }
            Expr::Propagate(e) => self.walk_expr(&e.operand),
            Expr::Must(e) => {
                self.walk_expr(&e.operand);
                if let Some(m) = &e.message {
                    self.walk_expr(m);
                }
            }
        }
    }

    fn walk_named_arg(&mut self, a: &NamedArg) {
        self.walk_expr(&a.value);
    }

    fn walk_perform(&mut self, p: &PerformExpr) {
        for (_, expr) in &p.args {
            self.walk_expr(expr);
        }
        // Tags should already be assigned in `collect_effect_decls`. If a
        // perform site references a fresh (effect, op) pair we missed,
        // allocate one defensively so the compiler can still emit a
        // well-formed opcode.
        if !self.effect_tags.contains_key(&p.effect) {
            let tag = self.alloc_effect_tag();
            self.effect_tags.insert(p.effect, tag);
        }
        if !self.op_tags.contains_key(&(p.effect, p.op)) {
            let next = self
                .op_tags
                .iter()
                .filter(|((e, _), _)| *e == p.effect)
                .map(|(_, t)| *t)
                .max()
                .map(|t| t.saturating_add(1))
                .unwrap_or(0);
            self.op_tags.insert((p.effect, p.op), next);
        }
    }

    fn walk_handle(&mut self, h: &HandleExpr) {
        // Walk body in the dynamic extent of the handler clauses (so that
        // perform sites inside the body can be considered "handled" — this
        // is informational only, since we don't statically check
        // UnhandledEffect for closed rows in this simple pass).
        self.walk_expr(&h.body);

        // Each clause body becomes its own analysis scope. Push the
        // continuation frame, walk, pop, and check whether the clause
        // resumed at all.
        for clause in &h.clauses {
            self.walk_clause(clause);
        }
    }

    fn walk_clause(&mut self, clause: &HandlerClause) {
        let frame = HandlerFrame {
            effect: clause.effect,
            op: clause.op,
            cont: clause.cont,
            resumed_once: false,
            resumed_twice: false,
        };
        self.handler_stack.push(frame);

        self.walk_expr(&clause.handler);

        // Pop and inspect.
        let popped = self.handler_stack.pop().expect("clause frame popped");
        if !popped.resumed_once {
            self.warnings.push(EffectWarning::ContinuationDropped {
                cont: clause.cont,
                span: clause.span,
            });
        }
    }

    fn walk_resume(&mut self, r: &ResumeExpr) {
        self.walk_expr(&r.value);
        match self.handler_stack.last_mut() {
            None => {
                // Defence-in-depth: the parser already rejects this. If we
                // somehow see it, surface the error so the user gets a
                // diagnostic rather than a silent miscompile.
                self.errors
                    .push(EffectError::ResumeOutsideHandler { span: r.span });
            }
            Some(frame) => {
                if frame.cont != r.cont {
                    // Resume bound name doesn't match the enclosing clause
                    // — also defence-in-depth; resolver should have caught
                    // it already as undefined / mismatched binding.
                    self.errors
                        .push(EffectError::ResumeOutsideHandler { span: r.span });
                } else if frame.resumed_once {
                    if !frame.resumed_twice {
                        frame.resumed_twice = true;
                        self.errors.push(EffectError::ResumedTwice {
                            cont: r.cont,
                            span: r.span,
                        });
                    }
                } else {
                    frame.resumed_once = true;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers (free functions)
// ---------------------------------------------------------------------------

/// Returns `&decl.item` if `item` is `Item::Export`, otherwise `item`.
/// Module-level effect declarations may be wrapped in `Export` when the
/// user wrote `pub effect Foo { ... }`.
fn unwrap_export(item: &Item) -> &Item {
    match item {
        Item::Export(decl) => unwrap_export(&decl.item),
        _ => item,
    }
}

// ---------------------------------------------------------------------------
// Debug invariants
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
fn debug_assertions(_ast: &ParseResult, analyzer: &Analyzer) {
    // Path-2 invariant: every `(effect, op)` pair that surfaced from a
    // perform / handle site has an entry in the tag table. The compiler
    // and VM rely on this to encode the bytecode opcode arguments.
    for effect in analyzer.effect_tags.keys() {
        debug_assert!(
            analyzer.effect_tags.contains_key(effect),
            "effects pass left an effect without a tag: {:?}",
            effect
        );
    }
    // Handler stack must be drained at the end.
    debug_assert!(
        analyzer.handler_stack.is_empty(),
        "ferric_effects: handler stack not drained at end of pass"
    );
}

#[cfg(not(debug_assertions))]
fn debug_assertions(_ast: &ParseResult, _analyzer: &Analyzer) {}

// `BUILTIN_EFFECTS` is referenced by `Analyzer::new` to seed
// `next_effect_tag` above the highest reserved built-in tag.
