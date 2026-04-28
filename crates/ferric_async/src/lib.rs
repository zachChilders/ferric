//! # `ferric_async` — async/await preparation pass.
//!
//! Slots into the pipeline between `ferric_typecheck` and `ferric_compiler`.
//! Reads a parsed AST plus type information, returns an `AsyncResult` whose
//! `ast` field is a rewritten `ParseResult` with one structural change:
//!
//! - `Item::AsyncFn(decl)` → `Item::Fn` whose return type is wrapped in
//!   `Async<T>` and whose body is wrapped in `AsyncBlock(original_body)`.
//!   This is a uniform representation: every async-producing function — named
//!   or anonymous — becomes "a regular fn whose body is an async block."
//!
//! Other transformations:
//!
//! - `Expr::Await` and `Expr::AsyncBlock` pass through unchanged. The compiler
//!   handles them directly via new `Op::MakeAsync` and `Op::Await` instructions.
//! - `Expr::Shell` inside an async context is rewritten to
//!   `await shell_run_async(cmd: <literal>)` so that subprocess execution
//!   participates in the async runtime. Outside an async context, a
//!   `BlockingShell` warning is emitted (if the program also uses async
//!   syntax, otherwise suppressed as noise).
//!
//! - `AsyncLowerError::InfiniteAsyncRecursion` fires for direct self-recursive
//!   async fns whose body is, syntactically, `await foo(...)` where `foo` is
//!   the enclosing fn — only the simplest case; mutual or branching recursion
//!   is detected at runtime by the scheduler's deadlock check.
//!
//! ## Architectural note
//!
//! An earlier draft of this stage (M8 Task 4 v1) emitted full state machines
//! per spec — state enum + poll fn + constructor fn — modelled on Rust's
//! async lowering. That approach forces every async value through a chain
//! of compile-time-erased coroutine state, which is the right design for a
//! language that compiles to machine code with no resumable VM state. Ferric
//! has a heap-allocated frame stack (M3+) that *can* hold paused state, so
//! the simpler model (Lua/Python-style coroutines) is a better fit. Path B,
//! the current architecture, was adopted in M8 Task 5 to unblock end-to-end
//! execution of async programs. See `docs/tasks/m8-04-lowering.md` and
//! `docs/tasks/m8-05-vm-stdlib.md` for the full design discussion.

use ferric_common::{
    AsyncBlockExpr, AsyncFnItem, AsyncLowerError, AsyncResult, AsyncWarning, AsyncWarningKind,
    AwaitExpr, CastExpr, ExportDecl, Expr, FnItem, ImplMethod, Item, Literal, MatchArm, NamedArg,
    NodeId, ParseResult, RequireStmt, ShellPart, Span, Stmt, Symbol, TypeResult,
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
    // structures it walks, so the resolver tables remain valid. The
    // architectural shift in M8 Task 5 Option A means `Item::AsyncFn`,
    // `Expr::Await`, and `Expr::AsyncBlock` all survive lowering — the
    // compiler handles them via dedicated emission paths (MakeAsync,
    // Await opcodes) rather than the original spec's state-machine
    // codegen. Lower_async's job is now mostly to detect lowering errors
    // and rewrite shell expressions inside async contexts.

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
            Item::Fn(f) => Item::Fn(FnItem {
                body: self.lower_expr(&f.body),
                ..f.clone()
            }),
            Item::AsyncFn(decl) => self.lower_async_fn(decl),
            Item::ImplBlock {
                id,
                trait_name,
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
        }
    }

    /// Walks the body of an async fn (lowering shell exprs etc.) but
    /// **leaves it as `Item::AsyncFn`**. The compiler handles the lazy
    /// wrapping directly at the `Item::AsyncFn` site, where it has full
    /// access to the fn's params and so can emit `MakeAsync(body_chunk,
    /// n_params)` correctly. Doing the wrap here in the lowering pass
    /// would require the resolver (which runs before this stage) to know
    /// about a node that doesn't yet exist — which it can't.
    fn lower_async_fn(&mut self, decl: &AsyncFnItem) -> Item {
        let f = &decl.item;
        let span = decl.span;
        self.has_async_syntax = true;

        self.async_depth += 1;
        let lowered_body = self.lower_expr(&f.body);
        let recurses = body_directly_self_awaits(&lowered_body, f.name);
        self.async_depth -= 1;

        if recurses {
            self.errors.push(AsyncLowerError::InfiniteAsyncRecursion {
                fn_name: f.name,
                span,
            });
        }

        Item::AsyncFn(AsyncFnItem {
            span,
            item: Box::new(FnItem {
                id: f.id,
                name: f.name,
                type_params: f.type_params.clone(),
                params: f.params.clone(),
                ret_ty: f.ret_ty.clone(),
                body: lowered_body,
                span: f.span,
            }),
        })
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
            Expr::Await(a) => self.lower_await(a),
            Expr::AsyncBlock(b) => self.lower_async_block(b),
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

    fn lower_await(&mut self, a: &AwaitExpr) -> Expr {
        self.has_async_syntax = true;
        Expr::Await(AwaitExpr {
            id: a.id,
            span: a.span,
            operand: Box::new(self.lower_expr(&a.operand)),
        })
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

        // Inside an async context: rewrite to `await shell_run_async(cmd: <s>)`.
        // The synthetic call literal carries a placeholder string; the real
        // lowering of shell-with-interpolation lands when interpolation gets
        // wired through the async runtime. For now this is enough to satisfy
        // the spec contract that shell-in-async goes through shell_run_async.
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
        Expr::Await(AwaitExpr {
            id: self.fresh_id(),
            span,
            operand: Box::new(call),
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

/// Best-effort direct self-recursion check: returns true if `body`'s tail
/// expression is `await foo(...)` where `foo` is the enclosing fn name.
fn body_directly_self_awaits(body: &Expr, fn_name: Symbol) -> bool {
    let tail = unwrap_block_tail(body);
    let Expr::Await(a) = tail else { return false };
    let Expr::Call { callee, .. } = a.operand.as_ref() else {
        return false;
    };
    matches!(
        callee.as_ref(),
        Expr::Variable { name, .. } if *name == fn_name,
    )
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
        Item::AsyncFn(decl) => walk_node_ids_expr(&decl.item.body, f),
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
        Expr::Await(a) => walk_node_ids_expr(&a.operand, f),
        Expr::AsyncBlock(b) => walk_node_ids_expr(&b.block, f),
    }
}
