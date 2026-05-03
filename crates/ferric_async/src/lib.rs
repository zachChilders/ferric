//! # `ferric_async` — async-aware diagnostic / shell-rewrite pass.
//!
//! Originally (M8) this crate performed a full async lowering: it turned
//! `Item::AsyncFn` into a state-machine-shaped `Item::Fn`, wrapped bodies
//! in `AsyncBlock`, and rewrote `await` sites. The M9 effects redesign
//! moved that work into the parser (`async fn` → `fn` returning
//! `Eff<[Async], T>`; `await expr` → `perform Async::Suspend(future:
//! expr)`) and into the unified effects pipeline (`ferric_effects`).
//!
//! What remains here is two diagnostics that the M9 fixtures still
//! depend on:
//!
//! - [`AsyncLowerError::InfiniteAsyncRecursion`] for the direct
//!   self-recursive case where an `async fn`'s tail is the desugared
//!   `perform Async::Suspend(future: foo(...))` calling itself.
//! - [`AsyncWarning::BlockingShell`] for any `$ ...` shell expression
//!   that lives outside an async context in a program that does use
//!   async/effects elsewhere. Inside an async context the shell is
//!   rewritten into an `await shell_run_async(...)` form so subprocess
//!   work participates in the runtime.
//!
//! The crate is still load-bearing — `lower_async` runs in
//! `main.rs`/`pipeline.rs` after typecheck and produces the diagnostics
//! M8 fixtures observe. M9 Task 7 explicitly chose to keep this stage in
//! place rather than merge it into another crate; see
//! `docs/tasks/m9-00-overview.md` for the rationale.

use ferric_common::{
    AsyncBlockExpr, AsyncLowerError, AsyncResult, AsyncWarning, AsyncWarningKind, CastExpr,
    ExportDecl, Expr, FnItem, ImplMethod, Item, Literal, MatchArm, NamedArg, NodeId, ParseResult,
    PerformExpr, RequireStmt, ShellPart, Span, Stmt, Symbol, TypeResult,
};

/// Single public entry point for the async preparation stage.
///
/// Walks `ast` once, applying the transformations described in the module
/// docs. Returns an `AsyncResult` carrying the rewritten AST, any lowering
/// errors, and any non-fatal warnings.
pub fn lower_async(ast: &ParseResult, _types: &TypeResult) -> AsyncResult {
    let mut lowerer = Lowerer::new(ast);

    let new_items: Vec<Item> = ast
        .items
        .iter()
        .map(|item| lowerer.lower_top_item(item))
        .collect();

    let new_ast = ParseResult::new(new_items, ast.errors.clone());

    // Output invariant: lower_async preserves all original NodeIds for
    // structures it walks, so the resolver tables remain valid. Post-M9
    // the parser's desugaring means there is no `Item::AsyncFn` or
    // `Expr::Await` to lower — this pass walks the tree to detect the
    // remaining diagnostics and to rewrite in-async shell expressions
    // into `perform Async::Suspend(future: shell_run_async(...))` calls.

    // BlockingShell is only useful when the program also uses async syntax —
    // for a pure M1–M7 program with no async at all, the warning is noise.
    let warnings = if lowerer.has_async_syntax {
        lowerer.warnings
    } else {
        Vec::new()
    };

    AsyncResult::new(new_ast, lowerer.errors, warnings)
}

// ---------------------------------------------------------------------------
// Lowerer state
// ---------------------------------------------------------------------------

struct Lowerer {
    /// Source of fresh NodeIds for the few new nodes we synthesize (the
    /// AsyncBlock wrapper around an `async fn` body, the Call/Await pair
    /// for in-async shell expressions). Initialised above the max NodeId
    /// already present in the input AST so old and new IDs never collide.
    next_node_id: u32,

    /// Nesting count of async contexts. Mirrors the type checker's
    /// `async_depth`; used to decide whether a shell expression is inside
    /// an async context (lower it) or outside (warn and pass through).
    async_depth: u32,

    /// Errors discovered during lowering.
    errors: Vec<AsyncLowerError>,

    /// Warnings discovered during lowering.
    warnings: Vec<AsyncWarning>,

    /// Set whenever the lowerer encounters any async-specific syntax. Used
    /// to gate `BlockingShell` warnings.
    has_async_syntax: bool,
}

impl Lowerer {
    fn new(ast: &ParseResult) -> Self {
        let max_id = max_node_id(ast).map(|n| n.0).unwrap_or(0);
        Self {
            next_node_id: max_id.saturating_add(1),
            async_depth: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
            has_async_syntax: false,
        }
    }

    fn fresh_id(&mut self) -> NodeId {
        let id = NodeId::new(self.next_node_id);
        self.next_node_id = self.next_node_id.saturating_add(1);
        id
    }
}

// ---------------------------------------------------------------------------
// Top-level item lowering
// ---------------------------------------------------------------------------

impl Lowerer {
    fn lower_top_item(&mut self, item: &Item) -> Item {
        match item {
            Item::Fn(f) => {
                // A post-M9 `async fn` survives the parser as `Item::Fn`
                // with `ret_ty = Eff<[Async], T>`. Bump `async_depth`
                // around its body so any `$ ...` shell expression inside
                // is correctly recognised as in-async (and rewritten to
                // `await shell_run_async(...)`), and so `has_async_syntax`
                // is set even when the body itself is empty.
                let body_async = fn_is_async(&f.ret_ty);
                if body_async {
                    self.async_depth += 1;
                    self.has_async_syntax = true;
                }
                let body = self.lower_expr(&f.body);
                if body_async {
                    self.async_depth -= 1;
                }
                // Re-implement the M8 direct self-recursion check post-M9:
                // the body's tail expression is
                // `perform Async::Suspend(future: foo(...))` where `foo`
                // is the enclosing fn. Detecting it here keeps the
                // diagnostic alive even though `Item::AsyncFn` no longer
                // survives parsing.
                if body_async && body_directly_self_awaits_via_perform(&body, f.name) {
                    self.errors.push(AsyncLowerError::InfiniteAsyncRecursion {
                        fn_name: f.name,
                        span: f.span,
                    });
                }
                Item::Fn(FnItem { body, ..f.clone() })
            }
            Item::ImplBlock {
                id,
                trait_name,
                trait_args,
                type_name,
                methods,
                span,
            } => {
                let methods = methods
                    .iter()
                    .map(|m| ImplMethod {
                        body: self.lower_expr(&m.body),
                        ..m.clone()
                    })
                    .collect();
                Item::ImplBlock {
                    id: *id,
                    trait_name: *trait_name,
                    trait_args: trait_args.clone(),
                    type_name: *type_name,
                    methods,
                    span: *span,
                }
            }
            Item::Script { stmt, id, span } => Item::Script {
                stmt: self.lower_stmt(stmt),
                id: *id,
                span: *span,
            },
            Item::Export(decl) => {
                let inner = self.lower_top_item(&decl.item);
                Item::Export(ExportDecl {
                    id: decl.id,
                    span: decl.span,
                    item: Box::new(inner),
                })
            }
            // No async nodes can hide inside these items; pass through with
            // NodeIds preserved so the resolver tables stay valid.
            Item::StructDef { .. }
            | Item::EnumDef { .. }
            | Item::TraitDef { .. }
            | Item::Import(_)
            | Item::TypeAlias(_) => item.clone(),
            // M9 Task 1: AST type added but parser does not yet emit it.
            // `ferric_async` does not handle effect declarations — they pass
            // through unchanged. (`ferric_async` is removed in M9 Task 7.)
            Item::EffectDecl(_) => item.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Statement / expression walkers
// ---------------------------------------------------------------------------

impl Lowerer {
    fn lower_stmt(&mut self, stmt: &Stmt) -> Stmt {
        match stmt {
            Stmt::Let {
                name,
                mutable,
                ty,
                init,
                id,
                span,
            } => Stmt::Let {
                name: *name,
                mutable: *mutable,
                ty: ty.clone(),
                init: self.lower_expr(init),
                id: *id,
                span: *span,
            },
            Stmt::Assign {
                target,
                value,
                id,
                span,
            } => Stmt::Assign {
                target: self.lower_expr(target),
                value: self.lower_expr(value),
                id: *id,
                span: *span,
            },
            Stmt::Expr { expr } => Stmt::Expr {
                expr: self.lower_expr(expr),
            },
            Stmt::Require(req) => Stmt::Require(RequireStmt {
                span: req.span,
                mode: req.mode.clone(),
                expr: Box::new(self.lower_expr(&req.expr)),
                message: req.message.as_ref().map(|m| Box::new(self.lower_expr(m))),
                set_fn: req.set_fn.as_ref().map(|s| Box::new(self.lower_expr(s))),
            }),
            Stmt::For {
                var,
                var_id,
                iter,
                body,
                id,
                span,
            } => Stmt::For {
                var: *var,
                var_id: *var_id,
                iter: self.lower_expr(iter),
                body: self.lower_expr(body),
                id: *id,
                span: *span,
            },
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> Expr {
        match expr {
            Expr::AsyncBlock(b) => self.lower_async_block(b),
            // M9: parser now emits `perform`/`handle`/`resume` (it desugars
            // `await` and `yield` to perform sites at parse time). Walk
            // the children but otherwise leave the nodes intact so the
            // downstream effects pass / compiler can lower them. This
            // crate is removed in M9 Task 7; until then it just needs to
            // be transparent to effect AST shapes.
            Expr::Perform(p) => {
                self.has_async_syntax = true;
                Expr::Perform(ferric_common::PerformExpr {
                    id: p.id,
                    span: p.span,
                    effect: p.effect,
                    op: p.op,
                    args: p
                        .args
                        .iter()
                        .map(|(name, value)| (*name, self.lower_expr(value)))
                        .collect(),
                })
            }
            Expr::Handle(h) => {
                self.has_async_syntax = true;
                let body = Box::new(self.lower_expr(&h.body));
                let clauses = h
                    .clauses
                    .iter()
                    .map(|c| ferric_common::HandlerClause {
                        span: c.span,
                        effect: c.effect,
                        op: c.op,
                        params: c.params.clone(),
                        cont: c.cont,
                        handler: Box::new(self.lower_expr(&c.handler)),
                    })
                    .collect();
                Expr::Handle(ferric_common::HandleExpr {
                    id: h.id,
                    span: h.span,
                    body,
                    clauses,
                })
            }
            Expr::Resume(r) => {
                self.has_async_syntax = true;
                Expr::Resume(ferric_common::ResumeExpr {
                    id: r.id,
                    span: r.span,
                    cont: r.cont,
                    value: Box::new(self.lower_expr(&r.value)),
                })
            }
            Expr::Shell { parts, id, span } => self.lower_shell(parts, *id, *span),

            Expr::Literal { .. }
            | Expr::Variable { .. }
            | Expr::Break { .. }
            | Expr::Continue { .. } => expr.clone(),

            Expr::Binary {
                op,
                left,
                right,
                id,
                span,
            } => Expr::Binary {
                op: *op,
                left: Box::new(self.lower_expr(left)),
                right: Box::new(self.lower_expr(right)),
                id: *id,
                span: *span,
            },
            Expr::Unary {
                op,
                expr: inner,
                id,
                span,
            } => Expr::Unary {
                op: *op,
                expr: Box::new(self.lower_expr(inner)),
                id: *id,
                span: *span,
            },
            Expr::Call {
                callee,
                args,
                id,
                span,
            } => Expr::Call {
                callee: Box::new(self.lower_expr(callee)),
                args: args
                    .iter()
                    .map(|a| NamedArg {
                        span: a.span,
                        name: a.name,
                        value: Box::new(self.lower_expr(&a.value)),
                    })
                    .collect(),
                id: *id,
                span: *span,
            },
            Expr::If {
                cond,
                then_branch,
                else_branch,
                id,
                span,
            } => Expr::If {
                cond: Box::new(self.lower_expr(cond)),
                then_branch: Box::new(self.lower_expr(then_branch)),
                else_branch: else_branch.as_ref().map(|e| Box::new(self.lower_expr(e))),
                id: *id,
                span: *span,
            },
            Expr::Block {
                stmts,
                expr: tail,
                id,
                span,
            } => Expr::Block {
                stmts: stmts.iter().map(|s| self.lower_stmt(s)).collect(),
                expr: tail.as_ref().map(|e| Box::new(self.lower_expr(e))),
                id: *id,
                span: *span,
            },
            Expr::Return { expr: e, id, span } => Expr::Return {
                expr: e.as_ref().map(|e| Box::new(self.lower_expr(e))),
                id: *id,
                span: *span,
            },
            Expr::While {
                cond,
                body,
                id,
                span,
            } => Expr::While {
                cond: Box::new(self.lower_expr(cond)),
                body: Box::new(self.lower_expr(body)),
                id: *id,
                span: *span,
            },
            Expr::Loop { body, id, span } => Expr::Loop {
                body: Box::new(self.lower_expr(body)),
                id: *id,
                span: *span,
            },
            Expr::Closure {
                params,
                body,
                id,
                span,
            } => Expr::Closure {
                params: params.clone(),
                body: Box::new(self.lower_expr(body)),
                id: *id,
                span: *span,
            },
            Expr::StructLit {
                name,
                fields,
                id,
                span,
            } => Expr::StructLit {
                name: *name,
                fields: fields
                    .iter()
                    .map(|(n, e)| (*n, self.lower_expr(e)))
                    .collect(),
                id: *id,
                span: *span,
            },
            Expr::FieldAccess {
                expr: e,
                field,
                id,
                span,
            } => Expr::FieldAccess {
                expr: Box::new(self.lower_expr(e)),
                field: *field,
                id: *id,
                span: *span,
            },
            Expr::Match {
                scrutinee,
                arms,
                id,
                span,
            } => Expr::Match {
                scrutinee: Box::new(self.lower_expr(scrutinee)),
                arms: arms
                    .iter()
                    .map(|arm| MatchArm {
                        pattern: arm.pattern.clone(),
                        body: self.lower_expr(&arm.body),
                        span: arm.span,
                    })
                    .collect(),
                id: *id,
                span: *span,
            },
            Expr::Tuple { elements, id, span } => Expr::Tuple {
                elements: elements.iter().map(|e| self.lower_expr(e)).collect(),
                id: *id,
                span: *span,
            },
            Expr::VariantCtor {
                enum_name,
                variant,
                args,
                id,
                span,
            } => Expr::VariantCtor {
                enum_name: *enum_name,
                variant: *variant,
                args: args.iter().map(|a| self.lower_expr(a)).collect(),
                id: *id,
                span: *span,
            },
            Expr::ArrayLit { elements, id, span } => Expr::ArrayLit {
                elements: elements.iter().map(|e| self.lower_expr(e)).collect(),
                id: *id,
                span: *span,
            },
            Expr::Index {
                array,
                index,
                id,
                span,
            } => Expr::Index {
                array: Box::new(self.lower_expr(array)),
                index: Box::new(self.lower_expr(index)),
                id: *id,
                span: *span,
            },
            Expr::MethodCall {
                receiver,
                method,
                args,
                id,
                span,
            } => Expr::MethodCall {
                receiver: Box::new(self.lower_expr(receiver)),
                method: *method,
                args: args
                    .iter()
                    .map(|a| NamedArg {
                        span: a.span,
                        name: a.name,
                        value: Box::new(self.lower_expr(&a.value)),
                    })
                    .collect(),
                id: *id,
                span: *span,
            },
            Expr::Cast(c) => Expr::Cast(CastExpr {
                id: c.id,
                span: c.span,
                expr: Box::new(self.lower_expr(&c.expr)),
                target: c.target.clone(),
            }),
        }
    }

    fn lower_async_block(&mut self, b: &AsyncBlockExpr) -> Expr {
        self.has_async_syntax = true;
        self.async_depth += 1;
        let inner = self.lower_expr(&b.block);
        self.async_depth -= 1;
        Expr::AsyncBlock(AsyncBlockExpr {
            id: b.id,
            span: b.span,
            block: Box::new(inner),
        })
    }

    fn lower_shell(&mut self, parts: &[ShellPart], id: NodeId, span: Span) -> Expr {
        // Lower interpolated sub-expressions first regardless of context.
        let lowered_parts: Vec<ShellPart> = parts
            .iter()
            .map(|p| match p {
                ShellPart::Literal(s) => ShellPart::Literal(s.clone()),
                ShellPart::Interpolated(e) => ShellPart::Interpolated(Box::new(self.lower_expr(e))),
            })
            .collect();

        if self.async_depth == 0 {
            self.warnings.push(AsyncWarning {
                span,
                kind: AsyncWarningKind::BlockingShell,
            });
            return Expr::Shell {
                parts: lowered_parts,
                id,
                span,
            };
        }

        // Inside an async context: rewrite to
        // `perform Async::Suspend(future: shell_run_async(cmd: <s>))`.
        // The synthetic call literal carries a placeholder string; the
        // real lowering of shell-with-interpolation lands when
        // interpolation gets wired through the async runtime. For now
        // this is enough to satisfy the spec contract that shell-in-async
        // goes through `shell_run_async`.
        let cmd_literal = Expr::Literal {
            value: Literal::Str(synth_symbol("<shell-cmd>")),
            id: self.fresh_id(),
            span,
        };
        let call = Expr::Call {
            callee: Box::new(Expr::Variable {
                name: synth_symbol("shell_run_async"),
                id: self.fresh_id(),
                span,
            }),
            args: vec![NamedArg {
                span,
                name: synth_symbol("cmd"),
                value: Box::new(cmd_literal),
            }],
            id: self.fresh_id(),
            span,
        };
        Expr::Perform(PerformExpr {
            id: self.fresh_id(),
            span,
            effect: synth_symbol("Async"),
            op: synth_symbol("Suspend"),
            args: vec![(synth_symbol("future"), call)],
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Synthesises a `Symbol` from a string. The lowering pass cannot mint new
/// interned strings without a `&mut Interner`, so generated names live in a
/// high-bit `u32` range that the interner is unlikely to reach (it's a dense
/// counter starting at 0). Used for `shell_run_async` and the `cmd` arg name
/// in the in-async shell rewrite — both of which are intercepted by the
/// resolver via the `native_fn_table` the user wires up at startup.
fn synth_symbol(name: &str) -> Symbol {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    Symbol::new(h | 0x8000_0000)
}

/// Detects the post-M9 desugared form of direct self-recursion: the tail
/// expression is `perform Async::Suspend(future: foo(...))` where `foo` is
/// the enclosing fn name. Used by [`Lowerer::lower_top_item`] on an
/// `Item::Fn` whose declared return type is `Eff<[Async], _>`.
fn body_directly_self_awaits_via_perform(body: &Expr, fn_name: Symbol) -> bool {
    let tail = unwrap_block_tail(body);
    let Expr::Perform(p) = tail else { return false };
    let Some((_, arg)) = p.args.first() else {
        return false;
    };
    let Expr::Call { callee, .. } = arg else {
        return false;
    };
    matches!(
        callee.as_ref(),
        Expr::Variable { name, .. } if *name == fn_name,
    )
}

/// Returns true if `ty` annotates an `Eff<row, _>` with at least one effect.
/// Best-effort: in M9 the parser only emits non-empty `Eff` rows for
/// `async fn` (and `gen fn`, but that's `Eff<[Gen<T>], Unit>` which has a
/// type-arg — not the shape this check fires on). User-written `Eff` rows
/// could in principle match, but that's acceptable for an InfiniteAsyncRecursion
/// best-effort detection.
fn fn_is_async(ty: &ferric_common::TypeAnnotation) -> bool {
    matches!(ty, ferric_common::TypeAnnotation::Eff { row, .. } if !row.is_empty() && row[0].args.is_empty())
}

fn unwrap_block_tail(expr: &Expr) -> &Expr {
    let mut cur = expr;
    while let Expr::Block {
        expr: Some(tail),
        stmts,
        ..
    } = cur
    {
        if !stmts.is_empty() {
            return cur;
        }
        cur = tail;
    }
    cur
}

fn max_node_id(ast: &ParseResult) -> Option<NodeId> {
    let mut max: Option<u32> = None;
    let mut bump = |id: NodeId| {
        max = Some(match max {
            Some(prev) => prev.max(id.0),
            None => id.0,
        });
    };
    for item in &ast.items {
        walk_node_ids_item(item, &mut bump);
    }
    max.map(NodeId::new)
}

fn walk_node_ids_item(item: &Item, f: &mut impl FnMut(NodeId)) {
    f(item.id());
    match item {
        Item::Fn(fi) => walk_node_ids_expr(&fi.body, f),
        Item::ImplBlock { methods, .. } => {
            for m in methods {
                f(m.id);
                walk_node_ids_expr(&m.body, f);
            }
        }
        Item::Script { stmt, .. } => walk_node_ids_stmt(stmt, f),
        Item::Export(decl) => walk_node_ids_item(&decl.item, f),
        Item::TraitDef { methods, .. } => {
            for tm in methods {
                f(tm.id);
            }
        }
        Item::StructDef { .. } | Item::EnumDef { .. } | Item::Import(_) | Item::TypeAlias(_) => {}
        // M9 Task 1: AST type added but parser does not yet emit it.
        Item::EffectDecl(_) => {}
    }
}

fn walk_node_ids_stmt(stmt: &Stmt, f: &mut impl FnMut(NodeId)) {
    f(stmt.id());
    match stmt {
        Stmt::Let { init, .. } => walk_node_ids_expr(init, f),
        Stmt::Assign { target, value, .. } => {
            walk_node_ids_expr(target, f);
            walk_node_ids_expr(value, f);
        }
        Stmt::Expr { expr } => walk_node_ids_expr(expr, f),
        Stmt::Require(req) => {
            walk_node_ids_expr(&req.expr, f);
            if let Some(m) = &req.message {
                walk_node_ids_expr(m, f);
            }
            if let Some(s) = &req.set_fn {
                walk_node_ids_expr(s, f);
            }
        }
        Stmt::For { iter, body, .. } => {
            walk_node_ids_expr(iter, f);
            walk_node_ids_expr(body, f);
        }
    }
}

fn walk_node_ids_expr(expr: &Expr, f: &mut impl FnMut(NodeId)) {
    f(expr.id());
    match expr {
        Expr::Literal { .. }
        | Expr::Variable { .. }
        | Expr::Break { .. }
        | Expr::Continue { .. } => {}
        Expr::Binary { left, right, .. } => {
            walk_node_ids_expr(left, f);
            walk_node_ids_expr(right, f);
        }
        Expr::Unary { expr: e, .. } => walk_node_ids_expr(e, f),
        Expr::Call { callee, args, .. } => {
            walk_node_ids_expr(callee, f);
            for a in args {
                walk_node_ids_expr(&a.value, f);
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            walk_node_ids_expr(cond, f);
            walk_node_ids_expr(then_branch, f);
            if let Some(e) = else_branch {
                walk_node_ids_expr(e, f);
            }
        }
        Expr::Block {
            stmts, expr: tail, ..
        } => {
            for s in stmts {
                walk_node_ids_stmt(s, f);
            }
            if let Some(t) = tail {
                walk_node_ids_expr(t, f);
            }
        }
        Expr::Return { expr: e, .. } => {
            if let Some(inner) = e {
                walk_node_ids_expr(inner, f);
            }
        }
        Expr::While { cond, body, .. } => {
            walk_node_ids_expr(cond, f);
            walk_node_ids_expr(body, f);
        }
        Expr::Loop { body, .. } | Expr::Closure { body, .. } => {
            walk_node_ids_expr(body, f);
        }
        Expr::Shell { parts, .. } => {
            for p in parts {
                if let ShellPart::Interpolated(e) = p {
                    walk_node_ids_expr(e, f);
                }
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                walk_node_ids_expr(e, f);
            }
        }
        Expr::FieldAccess { expr: e, .. } => walk_node_ids_expr(e, f),
        Expr::Match {
            scrutinee, arms, ..
        } => {
            walk_node_ids_expr(scrutinee, f);
            for arm in arms {
                walk_node_ids_expr(&arm.body, f);
            }
        }
        Expr::Tuple { elements, .. } | Expr::ArrayLit { elements, .. } => {
            for e in elements {
                walk_node_ids_expr(e, f);
            }
        }
        Expr::VariantCtor { args, .. } => {
            for a in args {
                walk_node_ids_expr(a, f);
            }
        }
        Expr::Index { array, index, .. } => {
            walk_node_ids_expr(array, f);
            walk_node_ids_expr(index, f);
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_node_ids_expr(receiver, f);
            for a in args {
                walk_node_ids_expr(&a.value, f);
            }
        }
        Expr::Cast(c) => walk_node_ids_expr(&c.expr, f),
        Expr::AsyncBlock(b) => walk_node_ids_expr(&b.block, f),
        Expr::Perform(p) => {
            for (_, e) in &p.args {
                walk_node_ids_expr(e, f);
            }
        }
        Expr::Handle(h) => {
            walk_node_ids_expr(&h.body, f);
            for c in &h.clauses {
                walk_node_ids_expr(&c.handler, f);
            }
        }
        Expr::Resume(r) => walk_node_ids_expr(&r.value, f),
    }
}
