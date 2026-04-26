//! # Ferric Compiler
//!
//! AST → bytecode lowering. Sits between type checking and the VM.
//!
//! Per Rule 1, this stage reads only `ParseResult`, `ResolveResult`, and
//! `TypeResult` from `ferric_common`; it never imports from the stage crates
//! themselves.
//!
//! ## Slot allocation
//!
//! `ResolveResult::def_slots` numbers every variable slot globally across the
//! whole program. The bytecode VM expects per-frame slot arrays starting at
//! 0, so the compiler keeps its own per-function `Symbol → u8` scope stack —
//! a Let or param allocates the next free local slot in the current chunk.
//! This sidesteps the global numbering and matches the shape the TreeWalker
//! already uses for its environment stack.
//!
//! ## Function references
//!
//! A user function reference is encoded as `Constant::Fn(chunk_idx)`, a native
//! reference as `Constant::NativeFn(symbol)`. Both are loaded with `LoadConst`
//! and invoked with `Op::Call(argc)`, dispatched at runtime on the popped
//! callable. The M3 spec lists `Op::CallNative` and `Op::TailCall`; both are
//! deferred and not used by this implementation.

use std::collections::HashMap;

use ferric_common::{
    BinOp, Chunk, Constant, DefId, Expr, ImplMethod, Interner, Item, Literal, MatchArm,
    NamedArg, NodeId, Op, ParseResult, Param, Pattern, Program, RequireMode, RequireStmt,
    ResolveResult, ShellPart, Stmt, Symbol, Ty, TypeResult, UnOp,
};

/// Name of the compiler-internal native that executes a shell command.
/// Duplicated in `ferric_stdlib::SHELL_EXEC_NATIVE` — kept in sync manually
/// because `ferric_compiler` cannot depend on any stage crate (Rule 3).
const SHELL_EXEC_NATIVE: &str = "__shell_exec";

/// Name of the stdlib native used to coerce `Int` interpolations into `Str`
/// inside a shell command. Also registered by `ferric_stdlib::register_stdlib`.
const INT_TO_STR_NATIVE: &str = "int_to_str";

/// Compiles an AST to a bytecode `Program`.
///
/// `ast` must already have passed resolution and type-checking; the compiler
/// trusts the metadata and does not re-validate. `interner` is read-only —
/// the compiler never mutates it, matching the stage-boundary contract.
pub fn compile(
    ast: &ParseResult,
    resolve: &ResolveResult,
    types: &TypeResult,
    interner: &Interner,
) -> Program {
    let mut compiler = Compiler::new(ast, resolve, types, interner);
    compiler.compile_program()
}

// ============================================================================
// Compiler
// ============================================================================

struct Compiler<'a> {
    ast: &'a ParseResult,
    resolve: &'a ResolveResult,
    types: &'a TypeResult,
    interner: &'a Interner,

    /// All compiled chunks. Index 0 is the entry chunk; user functions follow.
    chunks: Vec<Chunk>,

    /// Currently-emitting chunk index.
    current: usize,

    /// User function name → chunk index, populated in a pre-pass so forward
    /// references and recursion resolve before any body is compiled.
    fn_chunks: HashMap<Symbol, u16>,

    /// Impl method DefId → chunk index. Methods do not have free-standing
    /// names, so the compiler must reference them through their `DefId`.
    method_chunks: HashMap<DefId, u16>,

    /// Per-function scope stack of `name → local slot`. Reset on each chunk.
    scopes: Vec<HashMap<Symbol, u8>>,

    /// Next free local slot in the current chunk. Monotonic — slots are not
    /// reused when a block scope ends.
    next_local: u8,

    /// Active loop contexts for break/continue patching.
    loop_stack: Vec<LoopContext>,
}

struct LoopContext {
    /// Bytecode offset of the loop start (continue jumps here).
    start_offset: usize,
    /// Patch addresses for break jumps; resolved at loop end.
    break_jumps: Vec<usize>,
}

impl<'a> Compiler<'a> {
    fn new(
        ast: &'a ParseResult,
        resolve: &'a ResolveResult,
        types: &'a TypeResult,
        interner: &'a Interner,
    ) -> Self {
        Self {
            ast,
            resolve,
            types,
            interner,
            chunks: Vec::new(),
            current: 0,
            fn_chunks: HashMap::new(),
            method_chunks: HashMap::new(),
            scopes: Vec::new(),
            next_local: 0,
            loop_stack: Vec::new(),
        }
    }

    // ---- Top level ----------------------------------------------------------

    fn compile_program(&mut self) -> Program {
        // Entry chunk is always index 0; its name is intentionally Symbol(0).
        self.chunks.push(Chunk::new(Symbol::new(0)));
        let entry_idx: u16 = 0;

        // Pre-pass: assign chunk indices to user functions and impl methods.
        for item in &self.ast.items {
            match item {
                Item::FnDef { name, .. } => {
                    let chunk_idx = self.chunks.len() as u16;
                    self.chunks.push(Chunk::new(*name));
                    self.fn_chunks.insert(*name, chunk_idx);
                }
                Item::ImplBlock { methods, .. } => {
                    for m in methods {
                        let def_id = match self.resolve.method_def_ids.get(&m.id) {
                            Some(d) => *d,
                            None => continue,
                        };
                        let chunk_idx = self.chunks.len() as u16;
                        self.chunks.push(Chunk::new(m.name));
                        self.method_chunks.insert(def_id, chunk_idx);
                    }
                }
                _ => {}
            }
        }

        // Compile each function body.
        for item in &self.ast.items {
            match item {
                Item::FnDef { name, params, body, .. } => {
                    let chunk_idx = self.fn_chunks[name] as usize;
                    self.enter_chunk(chunk_idx);
                    self.push_scope();
                    for param in params {
                        self.bind_local(param.name);
                    }
                    self.compile_expr(body);
                    self.emit(Op::Return);
                    self.pop_scope();
                }
                Item::ImplBlock { methods, .. } => {
                    for m in methods {
                        self.compile_impl_method(m);
                    }
                }
                _ => {}
            }
        }

        // Compile top-level script statements into the entry chunk.
        self.enter_chunk(0);
        self.push_scope();

        let scripts: Vec<&Stmt> = self
            .ast
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Script { stmt, .. } => Some(stmt),
                _ => None,
            })
            .collect();
        let last = scripts.len().saturating_sub(1);

        for (i, stmt) in scripts.into_iter().enumerate() {
            let is_last = i == last;
            match stmt {
                Stmt::Expr { expr } => {
                    self.compile_expr(expr);
                    if !is_last {
                        self.emit(Op::Pop);
                    }
                }
                _ => self.compile_stmt(stmt),
            }
        }
        self.emit(Op::Return);
        self.pop_scope();

        Program::new(std::mem::take(&mut self.chunks), entry_idx)
    }

    // ---- Chunk management ---------------------------------------------------

    fn enter_chunk(&mut self, idx: usize) {
        self.current = idx;
        self.scopes.clear();
        self.next_local = 0;
    }

    fn current_chunk_mut(&mut self) -> &mut Chunk {
        &mut self.chunks[self.current]
    }

    fn current_offset(&self) -> usize {
        self.chunks[self.current].code.len()
    }

    fn emit(&mut self, op: Op) {
        self.current_chunk_mut().code.push(op);
    }

    fn emit_jump(&mut self, op: Op) -> usize {
        let addr = self.current_offset();
        self.emit(op);
        addr
    }

    /// Patches a previously-emitted jump to land at the current offset.
    /// Offsets are relative to the instruction *after* the jump.
    fn patch_jump(&mut self, addr: usize) {
        let target = self.current_offset() as i64 - addr as i64 - 1;
        let offset = i16::try_from(target).expect("jump exceeds i16 range");
        let chunk = self.current_chunk_mut();
        match &mut chunk.code[addr] {
            Op::Jump(o) | Op::JumpIfFalse(o) | Op::JumpIfTrue(o) => *o = offset,
            other => panic!("patch_jump on non-jump op: {:?}", other),
        }
    }

    /// Emits an unconditional backward jump to `target` (an absolute chunk
    /// offset).
    fn emit_backward_jump(&mut self, target: usize) {
        let from_after = self.current_offset() as i64 + 1;
        let raw = target as i64 - from_after;
        let offset = i16::try_from(raw).expect("backward jump exceeds i16 range");
        self.emit(Op::Jump(offset));
    }

    fn add_constant(&mut self, c: Constant) -> u8 {
        let chunk = self.current_chunk_mut();
        for (i, existing) in chunk.constants.iter().enumerate() {
            if existing == &c {
                return i as u8;
            }
        }
        let idx = chunk.constants.len();
        assert!(idx <= u8::MAX as usize, "constant pool overflow");
        chunk.constants.push(c);
        idx as u8
    }

    // ---- Scopes / locals ----------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Allocates a new local slot for `name` in the current scope and returns it.
    fn bind_local(&mut self, name: Symbol) -> u8 {
        let slot = self.next_local;
        self.next_local = self.next_local.checked_add(1).expect("local slot overflow");
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, slot);
        }
        slot
    }

    /// Looks up a local slot by name through the scope stack.
    fn lookup_local(&self, name: Symbol) -> Option<u8> {
        for scope in self.scopes.iter().rev() {
            if let Some(slot) = scope.get(&name) {
                return Some(*slot);
            }
        }
        None
    }

    // ---- Statements ---------------------------------------------------------

    fn compile_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, init, .. } => {
                self.compile_expr(init);
                let slot = self.bind_local(*name);
                self.emit(Op::StoreSlot(slot));
            }
            Stmt::Assign { target, value, .. } => {
                self.compile_expr(value);
                if let Expr::Variable { name, .. } = target {
                    if let Some(slot) = self.lookup_local(*name) {
                        self.emit(Op::StoreSlot(slot));
                        return;
                    }
                }
                // Unresolved target — drop the value to keep the stack balanced.
                self.emit(Op::Pop);
            }
            Stmt::Expr { expr } => {
                self.compile_expr(expr);
                self.emit(Op::Pop);
            }
            Stmt::Require(req) => self.compile_require(req),
            Stmt::For { var, iter, body, .. } => self.compile_for(*var, iter, body),
        }
    }

    /// Compiles `for x in arr { body }` as a counting loop:
    ///
    /// ```text
    /// let __arr = iter
    /// let mut __i = 0
    /// loop:
    ///     if __i >= __arr.len() { break }
    ///     let x = __arr[__i]
    ///     body; pop
    ///     __i = __i + 1
    /// ```
    fn compile_for(&mut self, var: Symbol, iter: &Expr, body: &Expr) {
        // Fresh scope for the loop's locals.
        self.push_scope();

        // arr_slot ← iter
        self.compile_expr(iter);
        let arr_slot = self.bind_anon_slot();
        self.emit(Op::StoreSlot(arr_slot));

        // i_slot ← 0
        let zero = self.add_constant(Constant::Int(0));
        self.emit(Op::LoadConst(zero));
        let i_slot = self.bind_anon_slot();
        self.emit(Op::StoreSlot(i_slot));

        // Loop top.
        let loop_start = self.current_offset();

        // i < len(arr)?  (push i, push arr, ArrayLen, LtInt → cond)
        self.emit(Op::LoadSlot(i_slot));
        self.emit(Op::LoadSlot(arr_slot));
        self.emit(Op::ArrayLen);
        self.emit(Op::LtInt);
        let exit_jump = self.emit_jump(Op::JumpIfFalse(0));

        self.loop_stack.push(LoopContext {
            start_offset: loop_start,
            break_jumps: Vec::new(),
        });

        // Bind `var` in a body scope so each iteration sees a fresh binding.
        self.push_scope();
        self.emit(Op::LoadSlot(arr_slot));
        self.emit(Op::LoadSlot(i_slot));
        self.emit(Op::ArrayGet);
        let var_slot = self.bind_local(var);
        self.emit(Op::StoreSlot(var_slot));

        // Compile body and discard its value.
        self.compile_expr(body);
        self.emit(Op::Pop);
        self.pop_scope();

        // i = i + 1
        self.emit(Op::LoadSlot(i_slot));
        let one = self.add_constant(Constant::Int(1));
        self.emit(Op::LoadConst(one));
        self.emit(Op::AddInt);
        self.emit(Op::StoreSlot(i_slot));

        self.emit_backward_jump(loop_start);
        self.patch_jump(exit_jump);

        let ctx = self.loop_stack.pop().unwrap();
        for addr in ctx.break_jumps {
            self.patch_jump(addr);
        }

        self.pop_scope();
    }

    fn compile_require(&mut self, req: &RequireStmt) {
        // Layout:
        //   [cond]
        //   JumpIfTrue → end
        //   (if set_fn:) [set_fn body; Pop] [cond] JumpIfTrue → end
        //   [message or ""]
        //   RequireFail | RequireWarn
        // end:

        // First condition test.
        self.compile_expr(&req.expr);
        let pass_jump_1 = self.emit_jump(Op::JumpIfTrue(0));

        // Optional recovery via set_fn closure, then second test.
        let pass_jump_2 = if let Some(set_fn) = &req.set_fn {
            // set_fn is always a zero-arg `Expr::Closure` produced by the
            // parser (`|| { ... }`). Inline the body directly — this preserves
            // the closure's ability to mutate outer locals (same chunk, same
            // scope) without needing real closure values.
            let body = match set_fn.as_ref() {
                Expr::Closure { body, .. } => body.as_ref(),
                other => panic!(
                    "require set_fn must be Expr::Closure, got {:?}",
                    std::mem::discriminant(other)
                ),
            };
            self.compile_expr(body);
            self.emit(Op::Pop);

            // Re-evaluate the condition.
            self.compile_expr(&req.expr);
            Some(self.emit_jump(Op::JumpIfTrue(0)))
        } else {
            None
        };

        // Failure path: push the message (or empty string as sentinel for
        // "no message supplied") and fail/warn.
        if let Some(msg) = &req.message {
            self.compile_expr(msg);
        } else {
            let idx = self.add_constant(Constant::Str(String::new()));
            self.emit(Op::LoadConst(idx));
        }
        match req.mode {
            RequireMode::Error => self.emit(Op::RequireFail),
            RequireMode::Warn => self.emit(Op::RequireWarn),
        }

        // Land here on either success path.
        self.patch_jump(pass_jump_1);
        if let Some(j) = pass_jump_2 {
            self.patch_jump(j);
        }
    }

    // ---- Expressions --------------------------------------------------------

    fn compile_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal { value, .. } => self.compile_literal(value),
            Expr::Variable { name, .. } => {
                if let Some(slot) = self.lookup_local(*name) {
                    self.emit(Op::LoadSlot(slot));
                    return;
                }
                if let Some(&chunk_idx) = self.fn_chunks.get(name) {
                    let idx = self.add_constant(Constant::Fn(chunk_idx));
                    self.emit(Op::LoadConst(idx));
                    return;
                }
                // Treat any other unresolved name as a native reference. The
                // resolver guarantees only known callables get this far.
                let idx = self.add_constant(Constant::NativeFn(*name));
                self.emit(Op::LoadConst(idx));
            }
            Expr::Binary { op, left, right, id, .. } => {
                self.compile_expr(left);
                self.compile_expr(right);
                self.emit_binop(*op, left.id(), *id);
            }
            Expr::Unary { op, expr, id, .. } => {
                self.compile_expr(expr);
                self.emit_unop(*op, *id);
            }
            Expr::Call { callee, args, id, .. } => self.compile_call(callee, args, *id),
            Expr::If { cond, then_branch, else_branch, .. } => {
                self.compile_expr(cond);
                let else_jump = self.emit_jump(Op::JumpIfFalse(0));
                self.compile_expr(then_branch);
                let end_jump = self.emit_jump(Op::Jump(0));
                self.patch_jump(else_jump);
                if let Some(else_br) = else_branch {
                    self.compile_expr(else_br);
                } else {
                    self.emit(Op::Unit);
                }
                self.patch_jump(end_jump);
            }
            Expr::Block { stmts, expr, .. } => {
                self.push_scope();
                for stmt in stmts {
                    self.compile_stmt(stmt);
                }
                if let Some(e) = expr {
                    self.compile_expr(e);
                } else {
                    self.emit(Op::Unit);
                }
                self.pop_scope();
            }
            Expr::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.compile_expr(e);
                } else {
                    self.emit(Op::Unit);
                }
                self.emit(Op::Return);
            }
            Expr::While { cond, body, .. } => {
                let loop_start = self.current_offset();
                self.compile_expr(cond);
                let exit_jump = self.emit_jump(Op::JumpIfFalse(0));
                self.loop_stack.push(LoopContext {
                    start_offset: loop_start,
                    break_jumps: Vec::new(),
                });
                self.compile_expr(body);
                self.emit(Op::Pop);
                self.emit_backward_jump(loop_start);
                self.patch_jump(exit_jump);
                let ctx = self.loop_stack.pop().unwrap();
                for addr in ctx.break_jumps {
                    self.patch_jump(addr);
                }
                self.emit(Op::Unit);
            }
            Expr::Loop { body, .. } => {
                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    start_offset: loop_start,
                    break_jumps: Vec::new(),
                });
                self.compile_expr(body);
                self.emit(Op::Pop);
                self.emit_backward_jump(loop_start);
                let ctx = self.loop_stack.pop().unwrap();
                for addr in ctx.break_jumps {
                    self.patch_jump(addr);
                }
                self.emit(Op::Unit);
            }
            Expr::Break { .. } => {
                let addr = self.emit_jump(Op::Jump(0));
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.break_jumps.push(addr);
                }
            }
            Expr::Continue { .. } => {
                let target = self.loop_stack.last().map(|c| c.start_offset).unwrap_or(0);
                self.emit_backward_jump(target);
            }
            Expr::Closure { params, body, id, .. } => {
                self.compile_closure_expr(*id, params, body);
            }
            Expr::Shell { parts, span, .. } => self.compile_shell(parts, *span),
            Expr::StructLit { name, fields, .. } => {
                // Push field values in declared (struct definition) order so
                // the runtime layout is stable regardless of the source order.
                let order = self.field_order(*name);
                for fname in &order {
                    let (_, fexpr) = fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .expect("missing field caught by resolver");
                    self.compile_expr(fexpr);
                }
                let n = u8::try_from(order.len()).expect("too many struct fields");
                self.emit(Op::MakeStruct(n));
            }
            Expr::FieldAccess { expr, field, .. } => {
                let recv_id = expr.id();
                self.compile_expr(expr);
                let recv_ty = self.types.node_types.get(&recv_id).cloned();
                if let Some(Ty::Struct { name, .. }) = recv_ty {
                    let idx = self.field_index(name, *field).unwrap_or(0);
                    self.emit(Op::GetField(idx));
                } else {
                    // Type checker should have rejected this; emit a no-op so
                    // codegen does not panic.
                    self.emit(Op::GetField(0));
                }
            }
            Expr::Tuple { elements, .. } => {
                for e in elements {
                    self.compile_expr(e);
                }
                let n = u8::try_from(elements.len()).expect("too many tuple elements");
                self.emit(Op::MakeTuple(n));
            }
            Expr::VariantCtor { enum_name, variant, args, .. } => {
                for a in args {
                    self.compile_expr(a);
                }
                let idx = self.variant_index(*enum_name, *variant).unwrap_or(0);
                let n = u8::try_from(args.len()).expect("too many variant fields");
                self.emit(Op::MakeVariant(idx, n));
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.compile_match(scrutinee, arms);
            }
            Expr::MethodCall { receiver, args, id, span, .. } => {
                self.compile_method_call(receiver, args, *id, *span);
            }
            Expr::ArrayLit { elements, .. } => {
                for e in elements {
                    self.compile_expr(e);
                }
                let n = u8::try_from(elements.len()).expect("array literal too large");
                self.emit(Op::MakeArray(n));
            }
            Expr::Index { array, index, .. } => {
                self.compile_expr(array);
                self.compile_expr(index);
                self.emit(Op::ArrayGet);
            }
            Expr::Cast(c) => {
                // Casts erase to a no-op — opaque types are a type-checker
                // artifact only. Just compile the underlying expression.
                self.compile_expr(&c.expr);
            }
        }
    }

    /// Compiles a user-syntax closure expression. The closure body becomes
    /// a fresh chunk; its leading slots hold the captured values, followed
    /// by the parameter slots. The caller emits `LoadSlot` for each capture
    /// (in capture-list order) followed by `MakeClosure(fn_idx, n)`.
    fn compile_closure_expr(
        &mut self,
        closure_id: NodeId,
        params: &[ferric_common::Param],
        body: &Expr,
    ) {
        // Look up the captures the resolver recorded for this closure. Empty
        // for the synthetic require-set closure (which compile_require inlines
        // directly and never reaches this path).
        let captures: Vec<(DefId, Symbol)> = self
            .resolve
            .captures
            .get(&closure_id)
            .cloned()
            .unwrap_or_default();

        // 1. In the *outer* chunk, push each captured value onto the stack so
        //    the runtime MakeClosure can pack them into the closure value.
        for (_def_id, name) in &captures {
            if let Some(slot) = self.lookup_local(*name) {
                self.emit(Op::LoadSlot(slot));
            } else {
                // Captured a top-level function or unknown — fall back to
                // Unit. The type checker should reject this case.
                self.emit(Op::Unit);
            }
        }

        // 2. Allocate a chunk for the closure body. Symbol(0) is the
        //    sentinel "anonymous" name (also used for the entry chunk).
        let chunk_idx = self.chunks.len() as u16;
        self.chunks.push(Chunk::new(Symbol::new(0)));

        // 3. Save the outer chunk's compilation state and switch to the new
        //    chunk so we can emit the closure body in isolation.
        let saved_current = self.current;
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_next_local = self.next_local;
        let saved_loop_stack = std::mem::take(&mut self.loop_stack);

        self.enter_chunk(chunk_idx as usize);
        self.push_scope();

        // Captures occupy the leading slots, in capture-list order.
        for (_def_id, name) in &captures {
            self.bind_local(*name);
        }
        // Params follow.
        for param in params {
            self.bind_local(param.name);
        }

        self.compile_expr(body);
        self.emit(Op::Return);
        self.pop_scope();

        // 4. Restore the outer state.
        self.current = saved_current;
        self.scopes = saved_scopes;
        self.next_local = saved_next_local;
        self.loop_stack = saved_loop_stack;

        // 5. Build the closure value at runtime.
        let n = u8::try_from(captures.len()).expect("too many captured variables");
        self.emit(Op::MakeClosure(chunk_idx, n));
    }

    /// Compiles a method call by looking up the resolved impl-method DefId
    /// (recorded by the type checker) and lowering to a regular function
    /// call where the receiver is the first argument.
    fn compile_method_call(
        &mut self,
        receiver: &Expr,
        args: &[NamedArg],
        id: NodeId,
        _span: ferric_common::Span,
    ) {
        // Push the receiver as the first argument.
        self.compile_expr(receiver);
        for arg in args {
            self.compile_expr(&arg.value);
        }

        // The type checker records which impl method to invoke.
        let def_id = self.types.method_dispatch.get(&id).copied();
        match def_id.and_then(|d| self.method_chunks.get(&d).copied()) {
            Some(chunk_idx) => {
                let cidx = self.add_constant(Constant::Fn(chunk_idx));
                self.emit(Op::LoadConst(cidx));
                let argc =
                    u8::try_from(args.len() + 1).expect("method call argc exceeds u8");
                self.emit(Op::Call(argc));
            }
            None => {
                // No dispatch info — type checker should have rejected this.
                // Pop pushed values to keep the stack balanced and push Unit.
                for _ in 0..(args.len() + 1) {
                    self.emit(Op::Pop);
                }
                self.emit(Op::Unit);
            }
        }
    }

    /// Compiles a single impl block method into its pre-allocated chunk.
    fn compile_impl_method(&mut self, m: &ImplMethod) {
        let def_id = match self.resolve.method_def_ids.get(&m.id) {
            Some(d) => *d,
            None => return,
        };
        let chunk_idx = match self.method_chunks.get(&def_id) {
            Some(c) => *c as usize,
            None => return,
        };
        self.enter_chunk(chunk_idx);
        self.push_scope();
        for param in &m.params {
            self.bind_local(param.name);
        }
        self.compile_expr(&m.body);
        self.emit(Op::Return);
        self.pop_scope();
    }

    // ---- M4 helpers ---------------------------------------------------------

    /// Returns the declared field names for a struct in source order.
    fn field_order(&self, struct_name: Symbol) -> Vec<Symbol> {
        let def_id = match self.resolve.type_defs.get(&struct_name) {
            Some(d) => *d,
            None => return Vec::new(),
        };
        self.resolve
            .struct_fields
            .get(&def_id)
            .map(|fs| fs.iter().map(|(n, _)| *n).collect())
            .unwrap_or_default()
    }

    /// Returns the index of `field` within `struct_name`'s declared fields.
    fn field_index(&self, struct_name: Symbol, field: Symbol) -> Option<u8> {
        let def_id = *self.resolve.type_defs.get(&struct_name)?;
        let fields = self.resolve.struct_fields.get(&def_id)?;
        let idx = fields.iter().position(|(n, _)| *n == field)?;
        u8::try_from(idx).ok()
    }

    /// Returns the index of `variant` within `enum_name`'s declared variants.
    fn variant_index(&self, enum_name: Symbol, variant: Symbol) -> Option<u16> {
        let def_id = *self.resolve.type_defs.get(&enum_name)?;
        let variants = self.resolve.enum_variants.get(&def_id)?;
        let idx = variants.iter().position(|(n, _)| *n == variant)?;
        u16::try_from(idx).ok()
    }

    /// Allocates an anonymous local slot (no name binding). Used for
    /// match-scrutinee temporaries and unpacked variant fields.
    fn bind_anon_slot(&mut self) -> u8 {
        let slot = self.next_local;
        self.next_local = self.next_local.checked_add(1).expect("local slot overflow");
        slot
    }

    /// Compiles a match expression. Each arm is tested in source order;
    /// a successful match runs the arm body, an unsuccessful one falls
    /// through to the next arm. If no arm matches, Unit is left on the
    /// stack — exhaustiveness checking should ensure this is unreachable.
    fn compile_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) {
        self.compile_expr(scrutinee);
        let scrut_slot = self.bind_anon_slot();
        self.emit(Op::StoreSlot(scrut_slot));

        let mut end_jumps: Vec<usize> = Vec::new();

        for arm in arms {
            self.push_scope();
            let mut fail_jumps: Vec<usize> = Vec::new();
            self.compile_pattern(scrut_slot, &arm.pattern, &mut fail_jumps);
            self.compile_expr(&arm.body);
            let end_jump = self.emit_jump(Op::Jump(0));
            end_jumps.push(end_jump);
            for j in fail_jumps {
                self.patch_jump(j);
            }
            self.pop_scope();
        }

        // Fallthrough — no arm matched. Push Unit. Exhaustiveness checking
        // should prevent this from being reachable in correct programs.
        self.emit(Op::Unit);

        for j in end_jumps {
            self.patch_jump(j);
        }
    }

    /// Emits code to test `pat` against the value in slot `slot`. On match,
    /// any sub-bindings are stored in newly allocated locals. On mismatch,
    /// emits a `JumpIfFalse` whose patch address is appended to `fail_jumps`.
    fn compile_pattern(&mut self, slot: u8, pat: &Pattern, fail_jumps: &mut Vec<usize>) {
        match pat {
            Pattern::Wildcard { .. } => {
                // Always succeeds, no binding.
            }
            Pattern::Variable { name, .. } => {
                // Bind the value in `slot` to a local under `name`. The
                // compiler tracks pattern-bound locals in its own scope stack
                // — distinct from `slot`, which holds the matched value.
                self.emit(Op::LoadSlot(slot));
                let var_slot = self.bind_local(*name);
                self.emit(Op::StoreSlot(var_slot));
            }
            Pattern::Literal { value, .. } => {
                self.emit(Op::LoadSlot(slot));
                self.compile_literal(value);
                self.emit_literal_eq(value);
                fail_jumps.push(self.emit_jump(Op::JumpIfFalse(0)));
            }
            Pattern::Tuple { patterns, .. } => {
                // Tuples have a fixed shape, so no test is needed — just
                // destructure into per-element slots and recurse.
                let elem_slots: Vec<u8> = (0..patterns.len())
                    .map(|_| self.bind_anon_slot())
                    .collect();
                for (i, &es) in elem_slots.iter().enumerate() {
                    self.emit(Op::LoadSlot(slot));
                    let idx = u8::try_from(i).expect("tuple element index out of range");
                    self.emit(Op::GetTupleField(idx));
                    self.emit(Op::StoreSlot(es));
                }
                for (sub, &es) in patterns.iter().zip(elem_slots.iter()) {
                    self.compile_pattern(es, sub, fail_jumps);
                }
            }
            Pattern::Struct { name, fields, .. } => {
                // Bind each named field to a fresh slot, then recurse.
                let mut sub_slots: Vec<(u8, &Pattern)> = Vec::new();
                for (fname, fpat) in fields {
                    let idx = self.field_index(*name, *fname).unwrap_or(0);
                    let s = self.bind_anon_slot();
                    self.emit(Op::LoadSlot(slot));
                    self.emit(Op::GetField(idx));
                    self.emit(Op::StoreSlot(s));
                    sub_slots.push((s, fpat));
                }
                for (s, sub) in sub_slots {
                    self.compile_pattern(s, sub, fail_jumps);
                }
            }
            Pattern::Variant {
                enum_name,
                variant,
                patterns,
                ..
            } => {
                let v_idx = self.variant_index(*enum_name, *variant).unwrap_or(0);
                self.emit(Op::LoadSlot(slot));
                self.emit(Op::MatchVariant(v_idx));
                fail_jumps.push(self.emit_jump(Op::JumpIfFalse(0)));

                if !patterns.is_empty() {
                    // Allocate one slot per payload field, then unpack
                    // (rightmost ends up on top of the stack, so store
                    // backwards).
                    let payload_slots: Vec<u8> = (0..patterns.len())
                        .map(|_| self.bind_anon_slot())
                        .collect();
                    self.emit(Op::LoadSlot(slot));
                    self.emit(Op::UnpackVariant);
                    for &s in payload_slots.iter().rev() {
                        self.emit(Op::StoreSlot(s));
                    }
                    for (sub, &s) in patterns.iter().zip(payload_slots.iter()) {
                        self.compile_pattern(s, sub, fail_jumps);
                    }
                }
            }
        }
    }

    /// Emits the typed equality op corresponding to a literal pattern.
    fn emit_literal_eq(&mut self, lit: &Literal) {
        match lit {
            Literal::Int(_) => self.emit(Op::EqInt),
            Literal::Float(_) => self.emit(Op::EqFloat),
            Literal::Bool(_) => self.emit(Op::EqBool),
            Literal::Str(_) => self.emit(Op::EqStr),
            Literal::Unit => {
                // Unit can only ever equal Unit; pop both, push true.
                self.emit(Op::Pop);
                self.emit(Op::Pop);
                let idx = self.add_constant(Constant::Bool(true));
                self.emit(Op::LoadConst(idx));
            }
        }
    }

    fn compile_shell(&mut self, parts: &[ShellPart], _span: ferric_common::Span) {
        // Build the command string, then call the compiler-internal native
        // `__shell_exec(cmd: Str) -> ShellOutput`.
        //
        // Each interpolated expression is coerced to `Str`:
        //   - `Str`   → push as-is
        //   - `Int`   → push, then call `int_to_str(n:)` native
        //   - other   → type checker should reject upstream; default to
        //               passing through as-is (runtime will complain at Concat)
        if parts.is_empty() {
            let idx = self.add_constant(Constant::Str(String::new()));
            self.emit(Op::LoadConst(idx));
        } else {
            let mut pushed = 0usize;
            for part in parts {
                match part {
                    ShellPart::Literal(s) => {
                        let idx = self.add_constant(Constant::Str(s.clone()));
                        self.emit(Op::LoadConst(idx));
                    }
                    ShellPart::Interpolated(expr) => {
                        self.compile_expr(expr);
                        let ty = self
                            .types
                            .node_types
                            .get(&expr.id())
                            .cloned()
                            .unwrap_or(Ty::Int);
                        if matches!(ty, Ty::Int) {
                            self.emit_native_call(INT_TO_STR_NATIVE, 1);
                        }
                    }
                }
                pushed += 1;
                if pushed >= 2 {
                    self.emit(Op::Concat);
                }
            }
        }

        // cmd string is now on top of the stack. Call __shell_exec(cmd).
        self.emit_native_call(SHELL_EXEC_NATIVE, 1);
    }

    /// Emits a call to a stdlib native by name. The named symbol must have
    /// been interned before `compile()` runs — `register_stdlib` does this
    /// for every stdlib native.
    fn emit_native_call(&mut self, name: &str, argc: u8) {
        let sym = self.interner.lookup(name).unwrap_or_else(|| {
            panic!(
                "native '{}' was not interned before compile() ran; ensure \
                 register_stdlib runs first",
                name
            )
        });
        let idx = self.add_constant(Constant::NativeFn(sym));
        self.emit(Op::LoadConst(idx));
        self.emit(Op::Call(argc));
    }

    fn compile_literal(&mut self, lit: &Literal) {
        match lit {
            Literal::Int(n) => {
                let idx = self.add_constant(Constant::Int(*n));
                self.emit(Op::LoadConst(idx));
            }
            Literal::Float(f) => {
                let idx = self.add_constant(Constant::Float(*f));
                self.emit(Op::LoadConst(idx));
            }
            Literal::Bool(b) => {
                let idx = self.add_constant(Constant::Bool(*b));
                self.emit(Op::LoadConst(idx));
            }
            Literal::Str(sym) => {
                // Resolve the interned string at compile time so the chunk
                // is self-contained and the VM does not need the interner
                // for literal strings at runtime.
                let s = self.interner.resolve(*sym).to_string();
                let idx = self.add_constant(Constant::Str(s));
                self.emit(Op::LoadConst(idx));
            }
            Literal::Unit => self.emit(Op::Unit),
        }
    }

    fn emit_binop(&mut self, op: BinOp, left_id: NodeId, _expr_id: NodeId) {
        let left_ty = self
            .types
            .node_types
            .get(&left_id)
            .cloned()
            .unwrap_or(Ty::Int);
        match op {
            BinOp::Add => match left_ty {
                Ty::Float => self.emit(Op::AddFloat),
                Ty::Str => self.emit(Op::Concat),
                _ => self.emit(Op::AddInt),
            },
            BinOp::Sub => match left_ty {
                Ty::Float => self.emit(Op::SubFloat),
                _ => self.emit(Op::SubInt),
            },
            BinOp::Mul => match left_ty {
                Ty::Float => self.emit(Op::MulFloat),
                _ => self.emit(Op::MulInt),
            },
            BinOp::Div => match left_ty {
                Ty::Float => self.emit(Op::DivFloat),
                _ => self.emit(Op::DivInt),
            },
            BinOp::Rem => self.emit(Op::RemInt),
            BinOp::Eq => match left_ty {
                Ty::Float => self.emit(Op::EqFloat),
                Ty::Bool => self.emit(Op::EqBool),
                Ty::Str => self.emit(Op::EqStr),
                _ => self.emit(Op::EqInt),
            },
            BinOp::Ne => match left_ty {
                Ty::Float => self.emit(Op::NeFloat),
                Ty::Bool => self.emit(Op::NeBool),
                Ty::Str => self.emit(Op::NeStr),
                _ => self.emit(Op::NeInt),
            },
            BinOp::Lt => match left_ty {
                Ty::Float => self.emit(Op::LtFloat),
                _ => self.emit(Op::LtInt),
            },
            BinOp::Le => match left_ty {
                Ty::Float => self.emit(Op::LeFloat),
                _ => self.emit(Op::LeInt),
            },
            BinOp::Gt => match left_ty {
                Ty::Float => self.emit(Op::GtFloat),
                _ => self.emit(Op::GtInt),
            },
            BinOp::Ge => match left_ty {
                Ty::Float => self.emit(Op::GeFloat),
                _ => self.emit(Op::GeInt),
            },
            BinOp::And => self.emit(Op::AndBool),
            BinOp::Or => self.emit(Op::OrBool),
        }
    }

    fn emit_unop(&mut self, op: UnOp, expr_id: NodeId) {
        let ty = self
            .types
            .node_types
            .get(&expr_id)
            .cloned()
            .unwrap_or(Ty::Int);
        match op {
            UnOp::Neg => match ty {
                Ty::Float => self.emit(Op::NegFloat),
                _ => self.emit(Op::NegInt),
            },
            UnOp::Not => self.emit(Op::Not),
        }
    }

    fn compile_call(&mut self, callee: &Expr, args: &[NamedArg], call_id: NodeId) {
        // Use canonical args (definition order, defaults inserted) when
        // available — every direct call to a known function has them.
        let canonical: Option<Vec<NamedArg>> =
            self.resolve.canonical_call_args.get(&call_id).cloned();
        let effective_args: &[NamedArg] = canonical.as_deref().unwrap_or(args);

        // Args left-to-right; the VM pops-and-reverses to recover order.
        for arg in effective_args {
            self.compile_expr(&arg.value);
        }

        // Push the callable last (Op::Call pops it first).
        self.compile_expr(callee);

        let argc = u8::try_from(effective_args.len()).expect("argc exceeds u8");
        self.emit(Op::Call(argc));
    }
}

// Silence unused-import warnings for symbols only used by future lowerings.
#[allow(dead_code)]
fn _unused_imports(_: Param) {}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::Interner;
    use ferric_lexer::lex;
    use ferric_parser::parse;
    use ferric_resolve::resolve_with_natives;
    use ferric_infer::typecheck;

    /// Drives the full pipeline (lex→parse→resolve→typecheck→compile) and
    /// returns the compiled `Program`.
    fn compile_source(src: &str) -> (Program, Interner) {
        let mut interner = Interner::new();
        // Intern every native the compiler may need to reference (shell
        // lowering uses `int_to_str` and `__shell_exec`).
        let native_fns: Vec<(Symbol, Vec<Symbol>)> = vec![
            (interner.intern("println"),       vec![interner.intern("s")]),
            (interner.intern("print"),         vec![interner.intern("s")]),
            (interner.intern("int_to_str"),    vec![interner.intern("n")]),
            (interner.intern("float_to_str"),  vec![interner.intern("n")]),
            (interner.intern("bool_to_str"),   vec![interner.intern("b")]),
            (interner.intern("int_to_float"),  vec![interner.intern("n")]),
            (interner.intern("shell_stdout"),  vec![interner.intern("output")]),
            (interner.intern("shell_exit_code"), vec![interner.intern("output")]),
        ];
        interner.intern("__shell_exec");
        let lex_result = lex(src, &mut interner);
        assert!(lex_result.errors.is_empty(), "lex errors: {:?}", lex_result.errors);
        let parse_result = parse(&lex_result);
        assert!(parse_result.errors.is_empty(), "parse errors: {:?}", parse_result.errors);
        let resolve_result = resolve_with_natives(&parse_result, &native_fns);
        assert!(resolve_result.errors.is_empty(), "resolve errors: {:?}", resolve_result.errors);
        let registry = ferric_common::TraitRegistry::new();
        let type_result = typecheck(&parse_result, &resolve_result, &interner, &registry);
        assert!(type_result.errors.is_empty(), "type errors: {:?}", type_result.errors);
        let program = compile(&parse_result, &resolve_result, &type_result, &interner);
        (program, interner)
    }

    fn entry_code(p: &Program) -> &[Op] {
        &p.chunks[p.entry as usize].code
    }

    #[test]
    fn integer_addition_uses_add_int() {
        let (program, _) = compile_source("1 + 2");
        let code = entry_code(&program);
        assert!(code.contains(&Op::AddInt), "expected AddInt: {:?}", code);
        assert!(!code.contains(&Op::AddFloat), "should not pick float: {:?}", code);
    }

    #[test]
    fn float_addition_uses_add_float() {
        let (program, _) = compile_source("1.0 + 2.0");
        let code = entry_code(&program);
        assert!(code.contains(&Op::AddFloat), "expected AddFloat: {:?}", code);
        assert!(!code.contains(&Op::AddInt), "should not pick int: {:?}", code);
    }

    #[test]
    fn string_concat_uses_concat_op() {
        let (program, _) = compile_source(r#""a" + "b""#);
        let code = entry_code(&program);
        assert!(code.contains(&Op::Concat), "expected Concat: {:?}", code);
    }

    #[test]
    fn let_then_use_emits_store_then_load_same_slot() {
        let (program, _) = compile_source("let x: Int = 5\nx");
        let code = entry_code(&program);
        let store = code.iter().find_map(|op| if let Op::StoreSlot(s) = op { Some(*s) } else { None });
        let load = code.iter().find_map(|op| if let Op::LoadSlot(s) = op { Some(*s) } else { None });
        assert_eq!(store, Some(0), "expected StoreSlot(0): {:?}", code);
        assert_eq!(load, Some(0), "expected LoadSlot(0): {:?}", code);
    }

    #[test]
    fn if_uses_jump_if_false_and_jump_to_skip_else() {
        let (program, _) = compile_source("if true { 1 } else { 2 }");
        let code = entry_code(&program);
        let has_jif = code.iter().any(|op| matches!(op, Op::JumpIfFalse(_)));
        let has_jmp = code.iter().any(|op| matches!(op, Op::Jump(_)));
        assert!(has_jif, "expected JumpIfFalse: {:?}", code);
        assert!(has_jmp, "expected Jump over else: {:?}", code);
    }

    #[test]
    fn while_loop_emits_backward_jump() {
        let (program, _) = compile_source("let mut i: Int = 0\nwhile i < 3 { i = i + 1 }");
        let code = entry_code(&program);
        // A while emits at least one negative-offset Jump (the backward jump).
        let has_back = code.iter().any(|op| matches!(op, Op::Jump(o) if *o < 0));
        assert!(has_back, "expected backward Jump: {:?}", code);
        let has_jif = code.iter().any(|op| matches!(op, Op::JumpIfFalse(_)));
        assert!(has_jif, "expected JumpIfFalse exit: {:?}", code);
    }

    #[test]
    fn break_in_loop_resolves_to_forward_jump() {
        let (program, _) = compile_source("loop { break }");
        let code = entry_code(&program);
        // The break must be a Jump with non-negative offset (forward).
        let break_jumps: Vec<i16> = code.iter().filter_map(|op| {
            if let Op::Jump(o) = op { Some(*o) } else { None }
        }).collect();
        assert!(break_jumps.iter().any(|o| *o >= 0), "expected forward Jump for break: {:?}", code);
    }

    #[test]
    fn function_definition_creates_separate_chunk() {
        let src = "fn add(a: Int, b: Int) -> Int { a + b }\nadd(a: 1, b: 2)";
        let (program, _) = compile_source(src);
        // Entry chunk + the function chunk.
        assert!(program.chunks.len() >= 2, "expected ≥2 chunks: {}", program.chunks.len());
        let fn_chunk = &program.chunks[1];
        assert!(fn_chunk.code.contains(&Op::AddInt), "fn body should AddInt: {:?}", fn_chunk.code);
        assert!(fn_chunk.code.contains(&Op::Return), "fn body should Return: {:?}", fn_chunk.code);
    }

    #[test]
    fn user_function_call_uses_constant_fn_then_call() {
        let src = "fn id(x: Int) -> Int { x }\nid(x: 7)";
        let (program, _) = compile_source(src);
        let entry = &program.chunks[program.entry as usize];
        assert!(matches!(entry.code.last(), Some(Op::Return)));
        let has_call = entry.code.iter().any(|op| matches!(op, Op::Call(1)));
        assert!(has_call, "expected Call(1): {:?}", entry.code);
        let has_fn_const = entry.constants.iter().any(|c| matches!(c, Constant::Fn(_)));
        assert!(has_fn_const, "expected Constant::Fn: {:?}", entry.constants);
    }

    #[test]
    fn native_call_uses_constant_native_fn() {
        let src = r#"println(s: "hi")"#;
        let (program, _) = compile_source(src);
        let entry = &program.chunks[program.entry as usize];
        let has_native = entry.constants.iter().any(|c| matches!(c, Constant::NativeFn(_)));
        assert!(has_native, "expected Constant::NativeFn: {:?}", entry.constants);
        let has_call = entry.code.iter().any(|op| matches!(op, Op::Call(1)));
        assert!(has_call, "expected Call(1): {:?}", entry.code);
    }

    #[test]
    fn constants_are_deduplicated() {
        let (program, _) = compile_source("1 + 1");
        let entry = &program.chunks[program.entry as usize];
        let int_ones = entry.constants.iter().filter(|c| matches!(c, Constant::Int(1))).count();
        assert_eq!(int_ones, 1, "expected 1 Constant::Int(1): {:?}", entry.constants);
    }

    #[test]
    fn unary_neg_picks_int_or_float_by_type() {
        // Use blocks so the unary minus isn't slurped into the let initializer.
        let (p_int, _) = compile_source("{ let x: Int = 5; -x }");
        let (p_float, _) = compile_source("{ let x: Float = 5.0; -x }");
        assert!(entry_code(&p_int).contains(&Op::NegInt), "{:?}", entry_code(&p_int));
        assert!(entry_code(&p_float).contains(&Op::NegFloat), "{:?}", entry_code(&p_float));
    }

    #[test]
    fn empty_program_still_has_entry_chunk() {
        let (program, _) = compile_source("");
        assert_eq!(program.chunks.len(), 1);
        assert_eq!(program.entry, 0);
        assert_eq!(program.chunks[0].code, vec![Op::Return]);
    }

    #[test]
    fn comparison_picks_typed_op() {
        let (p, _) = compile_source("1 < 2");
        assert!(entry_code(&p).contains(&Op::LtInt));
        let (p, _) = compile_source("1.0 < 2.0");
        assert!(entry_code(&p).contains(&Op::LtFloat));
    }
}
