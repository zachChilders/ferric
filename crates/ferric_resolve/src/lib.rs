//! # Ferric Name Resolution
//!
//! This crate performs name resolution (scope analysis) on the AST.
//! It catches undefined variables and duplicate definitions, assigns
//! DefIds to all definitions, and maps all variable uses to their definitions.
//!
//! Public API: Only the `resolve()` function is exposed.

use std::collections::HashMap;
use ferric_common::{
    ParseResult, ResolveResult, ResolveError, Item, ImplMethod, Stmt, Expr,
    NamedArg, Param, Pattern, ShellPart, Symbol, Span, NodeId, DefId, TypeAnnotation,
    RequireStmt,
};

/// Public entry point for name resolution.
///
/// Takes a ParseResult and produces a ResolveResult containing:
/// - Mappings from variable uses (NodeId) to definitions (DefId)
/// - Stack slot assignments for variables
/// - Function slot assignments for functions
/// - Canonicalized call argument lists (definition order, defaults inserted)
/// - Any resolution errors encountered
pub fn resolve(ast: &ParseResult) -> ResolveResult {
    resolve_with_natives(ast, &[])
}

/// Name resolution with support for native functions.
///
/// `native_fns` is a slice of `(fn_name, param_names)` pairs — one entry per
/// native function, providing the parameter names needed for named-arg validation.
pub fn resolve_with_natives(ast: &ParseResult, native_fns: &[(Symbol, Vec<Symbol>)]) -> ResolveResult {
    let mut resolver = Resolver::new();

    // Pre-register native functions (name in scope + param info for call validation)
    for (name, param_names) in native_fns {
        resolver.register_native(*name, param_names.clone());
    }

    resolver.resolve_program(ast);
    resolver.into_result()
}

// ============================================================================
// Private implementation below
// ============================================================================

/// Generator for unique DefIds.
struct DefIdGen {
    next: u32,
}

impl DefIdGen {
    fn new() -> Self {
        Self { next: 0 }
    }

    fn next(&mut self) -> DefId {
        let id = DefId(self.next);
        self.next += 1;
        id
    }
}

/// A binding in a scope.
struct Binding {
    def_id: DefId,
    mutable: bool,
    span: Span,
    /// `true` for native pre-registrations: user code may shadow the binding
    /// without producing a `DuplicateDefinition` error.
    shadowable: bool,
}

/// A scope containing variable bindings.
struct Scope {
    bindings: HashMap<Symbol, Binding>,
}

impl Scope {
    fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }
}

/// One frame for each closure currently being resolved. Lets the resolver
/// detect when a `Variable` lookup crosses into a captured outer scope.
struct ClosureFrame {
    id: NodeId,
    /// Number of scopes on the stack at the moment the closure was entered.
    /// A binding found in scopes [0, scope_floor) is a capture.
    scope_floor: usize,
    /// Captured (DefId, source name) pairs in the order they were first
    /// referenced. Deduplicated via `captured_set`.
    captures: Vec<(DefId, Symbol)>,
    captured_set: std::collections::HashSet<DefId>,
}

/// The name resolver.
struct Resolver {
    /// Stack of scopes (innermost is last)
    scopes: Vec<Scope>,
    /// Stack of in-progress closures (innermost last).
    closure_stack: Vec<ClosureFrame>,
    /// Output: for each closure NodeId, the captured (DefId, name) pairs in
    /// the order they were first referenced inside the closure body.
    captures: HashMap<NodeId, Vec<(DefId, Symbol)>>,

    /// DefId generator
    def_id_gen: DefIdGen,

    /// Next variable slot index
    next_slot: u32,

    /// Next function slot index
    next_fn_slot: u32,

    /// Output: maps NodeId (variable use) to DefId (definition)
    resolutions: HashMap<NodeId, DefId>,

    /// Output: maps DefId to variable stack slot
    def_slots: HashMap<DefId, u32>,

    /// Output: maps DefId to function slot
    fn_slots: HashMap<DefId, u32>,

    /// Output: maps Call NodeId to canonical arg list (definition order, defaults inserted)
    canonical_call_args: HashMap<NodeId, Vec<NamedArg>>,

    /// Known function parameter lists (user-defined + native), keyed by function name
    fn_params: HashMap<Symbol, Vec<Param>>,

    /// Output: maps struct/enum name to its DefId.
    type_defs: HashMap<Symbol, DefId>,
    /// Output: for each struct DefId, its declared fields in order.
    struct_fields: HashMap<DefId, Vec<(Symbol, TypeAnnotation)>>,
    /// Output: for each enum DefId, its declared variants in order.
    enum_variants: HashMap<DefId, Vec<(Symbol, Vec<TypeAnnotation>)>>,
    /// Output: each impl method's DefId, keyed by the method's NodeId.
    method_def_ids: HashMap<NodeId, DefId>,
    /// Output: each impl method's declared params, keyed by the method's NodeId.
    method_params: HashMap<NodeId, Vec<Param>>,

    /// Accumulated errors
    errors: Vec<ResolveError>,

    /// Depth of loop nesting (for validating break/continue)
    loop_depth: u32,

    /// Depth of function nesting (for validating return)
    fn_depth: u32,
}

impl Resolver {
    fn new() -> Self {
        Self {
            scopes: Vec::new(),
            closure_stack: Vec::new(),
            captures: HashMap::new(),
            def_id_gen: DefIdGen::new(),
            next_slot: 0,
            next_fn_slot: 0,
            resolutions: HashMap::new(),
            def_slots: HashMap::new(),
            fn_slots: HashMap::new(),
            canonical_call_args: HashMap::new(),
            fn_params: HashMap::new(),
            type_defs: HashMap::new(),
            struct_fields: HashMap::new(),
            enum_variants: HashMap::new(),
            method_def_ids: HashMap::new(),
            method_params: HashMap::new(),
            errors: Vec::new(),
            loop_depth: 0,
            fn_depth: 0,
        }
    }

    /// Consumes the resolver and produces the final ResolveResult.
    fn into_result(self) -> ResolveResult {
        let mut result = ResolveResult::new(
            self.resolutions,
            self.def_slots,
            self.fn_slots,
            self.canonical_call_args,
            self.errors,
        );
        result.type_defs = self.type_defs;
        result.struct_fields = self.struct_fields;
        result.enum_variants = self.enum_variants;
        result.method_def_ids = self.method_def_ids;
        result.method_params = self.method_params;
        result.captures = self.captures;
        result
    }

    /// Pushes a new scope onto the scope stack.
    fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    /// Pops the current scope off the scope stack.
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Defines a new binding in the current scope.
    ///
    /// Returns the DefId assigned to this definition.
    /// Reports a DuplicateDefinition error if the name is already defined in the current scope.
    fn define(&mut self, name: Symbol, mutable: bool, span: Span) -> DefId {
        let def_id = self.def_id_gen.next();

        // Check for duplicate definition in the current scope only
        if let Some(scope) = self.scopes.last_mut() {
            if let Some(existing) = scope.bindings.get(&name) {
                // Native pre-registrations are marked shadowable. Allow user
                // code to override them silently — otherwise adding a new
                // stdlib function could break unrelated user programs.
                if existing.shadowable {
                    scope.bindings.insert(
                        name,
                        Binding { def_id, mutable, span, shadowable: false },
                    );
                    return def_id;
                }
                // Duplicate definition error
                self.errors.push(ResolveError::DuplicateDefinition {
                    name,
                    first: existing.span,
                    second: span,
                });
            } else {
                // Insert the new binding
                scope.bindings.insert(name, Binding {
                    def_id,
                    mutable,
                    span,
                    shadowable: false,
                });
            }
        }

        def_id
    }

    /// Looks up a name in the scope stack (innermost to outermost).
    fn lookup(&self, name: Symbol) -> Option<&Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.bindings.get(&name) {
                return Some(binding);
            }
        }
        None
    }

    /// Like `lookup`, but also returns the index in `self.scopes` where the
    /// binding was found. Used to detect captured variables in closures.
    fn lookup_with_scope(&self, name: Symbol) -> Option<(&Binding, usize)> {
        for (idx, scope) in self.scopes.iter().enumerate().rev() {
            if let Some(binding) = scope.bindings.get(&name) {
                return Some((binding, idx));
            }
        }
        None
    }

    /// If we're currently resolving inside one or more closures, record `def_id`
    /// as a capture for every enclosing closure whose `scope_floor` is greater
    /// than `binding_scope`. This handles nested closures correctly.
    fn note_capture(&mut self, def_id: DefId, name: Symbol, binding_scope: usize) {
        for frame in self.closure_stack.iter_mut() {
            if binding_scope < frame.scope_floor && frame.captured_set.insert(def_id) {
                frame.captures.push((def_id, name));
            }
        }
    }

    /// Registers a native function as a pre-defined name with its parameter names.
    fn register_native(&mut self, name: Symbol, param_names: Vec<Symbol>) {
        // Ensure global scope exists
        if self.scopes.is_empty() {
            self.push_scope();
        }

        let def_id = self.def_id_gen.next();

        // Add to global scope (first scope in the stack). Mark shadowable so
        // a user-defined `fn name(...)` overrides it silently.
        if let Some(scope) = self.scopes.first_mut() {
            scope.bindings.insert(name, Binding {
                def_id,
                mutable: false,
                span: Span::new(0, 0),
                shadowable: true,
            });
        }

        // Assign a function slot to the native function
        let fn_slot = self.next_fn_slot;
        self.next_fn_slot += 1;
        self.fn_slots.insert(def_id, fn_slot);

        // Store parameter info for named-arg validation at call sites
        let params: Vec<Param> = param_names.into_iter().map(|pname| Param {
            span: Span::new(0, 0),
            name: pname,
            ty: TypeAnnotation::Named(Symbol::new(0)), // unused by resolver
            default: None,
        }).collect();
        self.fn_params.insert(name, params);
    }

    /// Resolves the entire program.
    fn resolve_program(&mut self, ast: &ParseResult) {
        // Create a top-level scope for the program (or reuse if natives were registered)
        if self.scopes.is_empty() {
            self.push_scope();
        }

        // Pre-pass: collect all user-defined function param info so calls to any
        // function (regardless of textual order) can be validated and canonicalized.
        // Also collect struct/enum definitions so type references in any function
        // (regardless of order) can be resolved.
        for item in &ast.items {
            match item {
                Item::FnDef { name, params, .. } => {
                    self.fn_params.insert(*name, params.clone());
                }
                Item::StructDef { name, fields, .. } => {
                    let def_id = self.def_id_gen.next();
                    self.type_defs.insert(*name, def_id);
                    self.struct_fields.insert(def_id, fields.clone());
                }
                Item::EnumDef { name, variants, .. } => {
                    let def_id = self.def_id_gen.next();
                    self.type_defs.insert(*name, def_id);
                    self.enum_variants.insert(def_id, variants.clone());
                }
                Item::TraitDef { name, .. } => {
                    // Reserve a DefId for the trait so name lookups can route
                    // through `type_defs` when needed.
                    let def_id = self.def_id_gen.next();
                    self.type_defs.insert(*name, def_id);
                }
                Item::ImplBlock { methods, .. } => {
                    // Each impl method gets its own DefId + fn_slot, plus the
                    // canonical-arg machinery used for named-call validation.
                    // Methods are not added to the global scope (they aren't
                    // free-standing names) — `Resolver::method_def_ids` indexes
                    // them by the method's NodeId so the type checker /
                    // compiler can find them later.
                    for m in methods {
                        let def_id = self.def_id_gen.next();
                        self.method_def_ids.insert(m.id, def_id);
                        let fn_slot = self.next_fn_slot;
                        self.next_fn_slot += 1;
                        self.fn_slots.insert(def_id, fn_slot);
                        self.method_params
                            .insert(m.id, m.params.clone());
                    }
                }
                Item::Script { .. } => {}
                // Module-system items are emitted starting with M7 Task 2; the
                // resolver does not yet collect their definitions.
                Item::Import(_) | Item::Export(_) | Item::TypeAlias(_) => {}
            }
        }

        for item in &ast.items {
            self.resolve_item(item);
        }

        self.pop_scope();
    }

    /// Resolves a top-level item.
    fn resolve_item(&mut self, item: &Item) {
        match item {
            Item::FnDef { name, params, body, span, .. } => {
                // Create DefId for the function
                let fn_def_id = self.define(*name, false, *span);

                // Assign function slot
                let fn_slot = self.next_fn_slot;
                self.next_fn_slot += 1;
                self.fn_slots.insert(fn_def_id, fn_slot);

                // Push a new scope for function body
                self.push_scope();

                // Increment function depth (we're inside a function now)
                self.fn_depth += 1;

                // Define parameters in the function scope
                for param in params {
                    let param_def_id = self.define(param.name, false, param.span);

                    // Assign variable slot to parameter
                    let slot = self.next_slot;
                    self.next_slot += 1;
                    self.def_slots.insert(param_def_id, slot);

                    // Resolve default expression (if any) in the outer scope — defaults
                    // are evaluated before the function body runs.
                    if let Some(default) = &param.default {
                        self.resolve_expr(default);
                    }
                }

                // Resolve function body
                self.resolve_expr(body);

                // Decrement function depth
                self.fn_depth -= 1;

                // Pop function scope
                self.pop_scope();
            }
            Item::StructDef { .. } | Item::EnumDef { .. } | Item::TraitDef { .. } => {
                // Type-level definitions: nothing to resolve in the body. The
                // pre-pass already registered the DefId and field/variant data.
            }
            Item::ImplBlock { methods, .. } => {
                for m in methods {
                    self.resolve_impl_method(m);
                }
            }
            Item::Script { stmt, .. } => {
                self.resolve_stmt(stmt);
            }
            // Module-system items: nothing to resolve in M7 Task 1 — the
            // module resolver runs as its own stage in Task 3.
            Item::Import(_) | Item::TypeAlias(_) => {}
            Item::Export(decl) => {
                self.resolve_item(&decl.item);
            }
        }
    }

    /// Resolves an `impl` block method body. The method has already been
    /// allocated a DefId + fn_slot in the pre-pass; this just walks the body
    /// and assigns parameter slots.
    fn resolve_impl_method(&mut self, m: &ImplMethod) {
        self.push_scope();
        self.fn_depth += 1;

        for param in &m.params {
            let pid = self.define(param.name, false, param.span);
            let slot = self.next_slot;
            self.next_slot += 1;
            self.def_slots.insert(pid, slot);
            if let Some(default) = &param.default {
                self.resolve_expr(default);
            }
        }

        self.resolve_expr(&m.body);

        self.fn_depth -= 1;
        self.pop_scope();
    }

    /// Resolves a statement.
    fn resolve_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, mutable, init, span, .. } => {
                // Resolve initializer first (before defining the variable)
                self.resolve_expr(init);

                // Define the variable in the current scope
                let def_id = self.define(*name, *mutable, *span);

                // Assign variable slot
                let slot = self.next_slot;
                self.next_slot += 1;
                self.def_slots.insert(def_id, slot);
            }
            Stmt::Assign { target, value, span, .. } => {
                // Resolve the value first
                self.resolve_expr(value);

                // Check that the target is a variable
                if let Expr::Variable { name, id, span: var_span } = target {
                    // Look up the variable and extract needed info
                    let binding_info = self.lookup(*name).map(|b| (b.def_id, b.mutable));

                    if let Some((def_id, mutable)) = binding_info {
                        // Check if it's mutable
                        if !mutable {
                            self.errors.push(ResolveError::AssignToImmutable {
                                name: *name,
                                span: *span,
                            });
                        }
                        // Record the resolution of the target variable
                        self.resolutions.insert(*id, def_id);
                    } else {
                        // Undefined variable
                        self.errors.push(ResolveError::UndefinedVariable {
                            name: *name,
                            span: *var_span,
                        });
                    }
                } else {
                    // For now, we only support assigning to simple variables
                    // In a full implementation, we might support field access, array indexing, etc.
                    // For M2, we just resolve the target expression
                    self.resolve_expr(target);
                }
            }
            Stmt::Expr { expr } => {
                self.resolve_expr(expr);
            }
            Stmt::Require(req) => {
                self.resolve_require(req);
            }
            Stmt::For { var, var_id, iter, body, span, .. } => {
                // Iterable is resolved in the outer scope.
                self.resolve_expr(iter);

                // The loop body sees a fresh scope binding `var`.
                self.push_scope();
                let def_id = self.define(*var, false, *span);
                let slot = self.next_slot;
                self.next_slot += 1;
                self.def_slots.insert(def_id, slot);
                self.resolutions.insert(*var_id, def_id);

                self.loop_depth += 1;
                self.resolve_expr(body);
                self.loop_depth -= 1;

                self.pop_scope();
            }
        }
    }

    /// Resolves a require statement.
    fn resolve_require(&mut self, req: &RequireStmt) {
        self.resolve_expr(&req.expr);

        if let Some(msg) = &req.message {
            self.resolve_expr(msg);
        }

        if let Some(set_fn) = &req.set_fn {
            // Check arity: set closure must have zero declared parameters
            if let Expr::Closure { params, span, .. } = set_fn.as_ref() {
                if !params.is_empty() {
                    self.errors.push(ResolveError::RequireSetArity { span: *span });
                }
            }
            // Resolve the set_fn expression
            self.resolve_expr(set_fn);
        }
    }

    /// Resolves an expression.
    fn resolve_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal { .. } => {
                // Literals don't need resolution
            }
            Expr::Variable { name, id, span } => {
                // Look up the variable in the scope stack
                if let Some((binding, scope_idx)) = self.lookup_with_scope(*name) {
                    // Record the resolution
                    let def_id = binding.def_id;
                    self.resolutions.insert(*id, def_id);
                    // If we're inside a closure and the binding lives in an
                    // outer scope, add it to the closure's capture list.
                    self.note_capture(def_id, *name, scope_idx);
                } else {
                    // Undefined variable error
                    self.errors.push(ResolveError::UndefinedVariable {
                        name: *name,
                        span: *span,
                    });
                }
            }
            Expr::Binary { left, right, .. } => {
                self.resolve_expr(left);
                self.resolve_expr(right);
            }
            Expr::Unary { expr, .. } => {
                self.resolve_expr(expr);
            }
            Expr::Call { callee, args, id, span } => {
                self.resolve_expr(callee);

                // Resolve all arg values first
                for arg in args {
                    self.resolve_expr(&arg.value);
                }

                // Named-arg validation and canonicalization (only for direct fn calls)
                if let Expr::Variable { name: fname, .. } = callee.as_ref() {
                    if let Some(params) = self.fn_params.get(fname).cloned() {
                        // Check for unknown arg names
                        for arg in args {
                            if !params.iter().any(|p| p.name == arg.name) {
                                self.errors.push(ResolveError::UnknownArg {
                                    name: arg.name,
                                    span: arg.span,
                                });
                            }
                        }

                        // Build canonical arg list in definition order
                        let mut canonical: Vec<NamedArg> = Vec::new();
                        for param in &params {
                            if let Some(arg) = args.iter().find(|a| a.name == param.name) {
                                canonical.push(arg.clone());
                            } else if let Some(default) = &param.default {
                                canonical.push(NamedArg {
                                    span: *span,
                                    name: param.name,
                                    value: default.clone(),
                                });
                            } else {
                                self.errors.push(ResolveError::MissingArg {
                                    param: param.name,
                                    call_span: *span,
                                });
                            }
                        }

                        self.canonical_call_args.insert(*id, canonical);
                    }
                }
            }
            Expr::If { cond, then_branch, else_branch, .. } => {
                self.resolve_expr(cond);
                self.resolve_expr(then_branch);
                if let Some(else_expr) = else_branch {
                    self.resolve_expr(else_expr);
                }
            }
            Expr::Block { stmts, expr, .. } => {
                // Push a new scope for the block
                self.push_scope();

                for stmt in stmts {
                    self.resolve_stmt(stmt);
                }

                if let Some(e) = expr {
                    self.resolve_expr(e);
                }

                // Pop the block scope
                self.pop_scope();
            }
            Expr::Return { expr, span, .. } => {
                // Check if we're inside a function
                if self.fn_depth == 0 {
                    self.errors.push(ResolveError::ReturnOutsideFn {
                        span: *span,
                    });
                }
                if let Some(e) = expr {
                    self.resolve_expr(e);
                }
            }
            Expr::While { cond, body, .. } => {
                self.resolve_expr(cond);

                // Increment loop depth before resolving body
                self.loop_depth += 1;
                self.resolve_expr(body);
                self.loop_depth -= 1;
            }
            Expr::Loop { body, .. } => {
                // Increment loop depth before resolving body
                self.loop_depth += 1;
                self.resolve_expr(body);
                self.loop_depth -= 1;
            }
            Expr::Break { span, .. } => {
                // Check if we're inside a loop
                if self.loop_depth == 0 {
                    self.errors.push(ResolveError::BreakOutsideLoop {
                        span: *span,
                    });
                }
            }
            Expr::Continue { span, .. } => {
                // Check if we're inside a loop
                if self.loop_depth == 0 {
                    self.errors.push(ResolveError::ContinueOutsideLoop {
                        span: *span,
                    });
                }
            }
            Expr::Closure { params, body, id, .. } => {
                // Push a scope for the closure body. Anything resolved against
                // a binding in scopes < scope_floor is a capture.
                self.push_scope();
                let scope_floor = self.scopes.len();
                self.closure_stack.push(ClosureFrame {
                    id: *id,
                    scope_floor,
                    captures: Vec::new(),
                    captured_set: std::collections::HashSet::new(),
                });

                // Define any parameters (for M2.5 set_fn closures these will be empty).
                for param in params {
                    let param_def_id = self.define(param.name, false, param.span);
                    let slot = self.next_slot;
                    self.next_slot += 1;
                    self.def_slots.insert(param_def_id, slot);
                }

                // Resolve the closure body in the closure's scope
                self.resolve_expr(body);

                let frame = self.closure_stack.pop().expect("closure frame popped");
                self.captures.insert(frame.id, frame.captures);
                self.pop_scope();
            }
            Expr::Shell { parts, .. } => {
                for part in parts {
                    if let ShellPart::Interpolated(expr) = part {
                        self.resolve_expr(expr);
                    }
                }
            }
            Expr::StructLit { name, fields, span, .. } => {
                let def_id = self.type_defs.get(name).copied();
                if def_id.is_none() {
                    self.errors.push(ResolveError::UndefinedType {
                        name: *name,
                        span: *span,
                    });
                }
                let declared_fields: Vec<Symbol> = def_id
                    .and_then(|id| self.struct_fields.get(&id))
                    .map(|fs| fs.iter().map(|(n, _)| *n).collect())
                    .unwrap_or_default();

                let mut seen: std::collections::HashSet<Symbol> = std::collections::HashSet::new();
                for (fname, fexpr) in fields {
                    self.resolve_expr(fexpr);
                    seen.insert(*fname);
                    if def_id.is_some() && !declared_fields.contains(fname) {
                        self.errors.push(ResolveError::UnknownField {
                            struct_name: *name,
                            field: *fname,
                            span: fexpr.span(),
                        });
                    }
                }
                for declared in &declared_fields {
                    if !seen.contains(declared) {
                        self.errors.push(ResolveError::MissingField {
                            struct_name: *name,
                            field: *declared,
                            span: *span,
                        });
                    }
                }
            }
            Expr::FieldAccess { expr, .. } => {
                self.resolve_expr(expr);
                // Field-existence checking is done in the type checker, where
                // we know the type of `expr`.
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.resolve_expr(scrutinee);
                for arm in arms {
                    // Each arm gets its own scope for pattern bindings.
                    self.push_scope();
                    self.resolve_pattern(&arm.pattern);
                    self.resolve_expr(&arm.body);
                    self.pop_scope();
                }
            }
            Expr::Tuple { elements, .. } => {
                for e in elements {
                    self.resolve_expr(e);
                }
            }
            Expr::VariantCtor { enum_name, variant, args, span, .. } => {
                self.resolve_variant_ref(*enum_name, *variant, *span, args.len());
                for a in args {
                    self.resolve_expr(a);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.resolve_expr(receiver);
                // Method-name → impl resolution is deferred to the type
                // checker (it depends on the receiver type). Here we just
                // resolve each argument expression.
                for a in args {
                    self.resolve_expr(&a.value);
                }
            }
            Expr::ArrayLit { elements, .. } => {
                for e in elements {
                    self.resolve_expr(e);
                }
            }
            Expr::Index { array, index, .. } => {
                self.resolve_expr(array);
                self.resolve_expr(index);
            }
            Expr::Cast(c) => {
                // The target type expression is resolved by later stages; here
                // we only need to walk the inner expression.
                self.resolve_expr(&c.expr);
            }
        }
    }

    /// Validates that `EnumName::Variant(...)` refers to a real variant with
    /// the right arity. Pattern matching reuses this same path.
    fn resolve_variant_ref(
        &mut self,
        enum_name: Symbol,
        variant: Symbol,
        span: Span,
        arity: usize,
    ) {
        match self.type_defs.get(&enum_name).copied() {
            None => {
                self.errors.push(ResolveError::UndefinedType {
                    name: enum_name,
                    span,
                });
            }
            Some(def_id) => match self.enum_variants.get(&def_id) {
                None => {
                    // Name resolves to a struct (or other type), not an enum.
                    self.errors.push(ResolveError::UnknownVariant {
                        enum_name,
                        variant,
                        span,
                    });
                }
                Some(variants) => {
                    match variants.iter().find(|(v, _)| *v == variant) {
                        None => {
                            self.errors.push(ResolveError::UnknownVariant {
                                enum_name,
                                variant,
                                span,
                            });
                        }
                        Some((_, payload)) => {
                            if payload.len() != arity {
                                self.errors.push(ResolveError::VariantArity {
                                    enum_name,
                                    variant,
                                    expected: payload.len(),
                                    found: arity,
                                    span,
                                });
                            }
                        }
                    }
                }
            },
        }
    }

    /// Resolves a pattern, defining any variable bindings in the current
    /// (innermost) scope.
    fn resolve_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Wildcard { .. } | Pattern::Literal { .. } => {}
            Pattern::Variable { name, id, span } => {
                let def_id = self.define(*name, false, *span);
                let slot = self.next_slot;
                self.next_slot += 1;
                self.def_slots.insert(def_id, slot);
                self.resolutions.insert(*id, def_id);
            }
            Pattern::Tuple { patterns, .. } => {
                for p in patterns {
                    self.resolve_pattern(p);
                }
            }
            Pattern::Struct { name, fields, span } => {
                let def_id = self.type_defs.get(name).copied();
                let declared_fields: Vec<Symbol> = def_id
                    .and_then(|id| self.struct_fields.get(&id))
                    .map(|fs| fs.iter().map(|(n, _)| *n).collect())
                    .unwrap_or_default();
                if def_id.is_none() {
                    self.errors.push(ResolveError::UndefinedType {
                        name: *name,
                        span: *span,
                    });
                }
                for (fname, fpat) in fields {
                    if def_id.is_some() && !declared_fields.contains(fname) {
                        self.errors.push(ResolveError::UnknownField {
                            struct_name: *name,
                            field: *fname,
                            span: fpat.span(),
                        });
                    }
                    self.resolve_pattern(fpat);
                }
            }
            Pattern::Variant { enum_name, variant, patterns, span } => {
                self.resolve_variant_ref(*enum_name, *variant, *span, patterns.len());
                for p in patterns {
                    self.resolve_pattern(p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Span, Symbol, NodeId, Item, Stmt, Expr, Literal, Param, TypeAnnotation};

    fn make_span() -> Span {
        Span::new(0, 0)
    }

    fn make_sym(n: u32) -> Symbol {
        Symbol(n)
    }

    fn make_node_id(n: u32) -> NodeId {
        NodeId(n)
    }

    #[test]
    fn test_simple_variable_resolution() {
        // let x = 5; x
        let ast = ParseResult::new(
            vec![
                Item::Script {
                    stmt: Stmt::Let {
                        name: make_sym(0), // x
                        mutable: false,
                        ty: None,
                        init: Expr::Literal {
                            value: Literal::Int(5),
                            id: make_node_id(0),
                            span: make_span(),
                        },
                        id: make_node_id(1),
                        span: make_span(),
                    },
                    id: make_node_id(2),
                    span: make_span(),
                },
                Item::Script {
                    stmt: Stmt::Expr {
                        expr: Expr::Variable {
                            name: make_sym(0), // x
                            id: make_node_id(3),
                            span: make_span(),
                        },
                    },
                    id: make_node_id(4),
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 0);
        assert!(result.resolutions.contains_key(&make_node_id(3)));
        assert_eq!(result.def_slots.len(), 1);
    }

    fn make_param(name: Symbol, ty: TypeAnnotation) -> Param {
        Param { span: Span::new(0, 0), name, ty, default: None }
    }

    #[test]
    fn test_function_parameter_resolution() {
        // fn foo(x: Int) { x }
        let ast = ParseResult::new(
            vec![
                Item::FnDef {
                    id: make_node_id(0),
                    name: make_sym(0), // foo
                    type_params: vec![],
                    params: vec![make_param(make_sym(1), TypeAnnotation::Named(make_sym(2)))],
                    ret_ty: TypeAnnotation::Named(make_sym(2)), // Int
                    body: Expr::Variable {
                        name: make_sym(1), // x
                        id: make_node_id(1),
                        span: make_span(),
                    },
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 0);
        assert!(result.resolutions.contains_key(&make_node_id(1)));
        assert_eq!(result.fn_slots.len(), 1);
    }

    #[test]
    fn test_shadowing() {
        // let x = 1; { let x = 2; x }
        let ast = ParseResult::new(
            vec![
                Item::Script {
                    stmt: Stmt::Let {
                        name: make_sym(0), // x
                        mutable: false,
                        ty: None,
                        init: Expr::Literal {
                            value: Literal::Int(1),
                            id: make_node_id(0),
                            span: make_span(),
                        },
                        id: make_node_id(1),
                        span: make_span(),
                    },
                    id: make_node_id(2),
                    span: make_span(),
                },
                Item::Script {
                    stmt: Stmt::Expr {
                        expr: Expr::Block {
                            stmts: vec![
                                Stmt::Let {
                                    name: make_sym(0), // x (shadows outer x)
                                    mutable: false,
                                    ty: None,
                                    init: Expr::Literal {
                                        value: Literal::Int(2),
                                        id: make_node_id(3),
                                        span: make_span(),
                                    },
                                    id: make_node_id(4),
                                    span: make_span(),
                                },
                            ],
                            expr: Some(Box::new(Expr::Variable {
                                name: make_sym(0), // x
                                id: make_node_id(5),
                                span: make_span(),
                            })),
                            id: make_node_id(6),
                            span: make_span(),
                        },
                    },
                    id: make_node_id(7),
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 0);
        // The variable use should resolve to the inner definition
        assert!(result.resolutions.contains_key(&make_node_id(5)));
        // There should be 2 definitions (outer x and inner x)
        assert_eq!(result.def_slots.len(), 2);
    }

    #[test]
    fn test_undefined_variable() {
        // let x = y (y is undefined)
        let ast = ParseResult::new(
            vec![
                Item::Script {
                    stmt: Stmt::Let {
                        name: make_sym(0), // x
                        mutable: false,
                        ty: None,
                        init: Expr::Variable {
                            name: make_sym(1), // y (undefined)
                            id: make_node_id(0),
                            span: make_span(),
                        },
                        id: make_node_id(1),
                        span: make_span(),
                    },
                    id: make_node_id(2),
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(
            result.errors[0],
            ResolveError::UndefinedVariable { .. }
        ));
    }

    #[test]
    fn test_duplicate_definition() {
        // let x = 1; let x = 2 (duplicate)
        let ast = ParseResult::new(
            vec![
                Item::Script {
                    stmt: Stmt::Let {
                        name: make_sym(0), // x
                        mutable: false,
                        ty: None,
                        init: Expr::Literal {
                            value: Literal::Int(1),
                            id: make_node_id(0),
                            span: make_span(),
                        },
                        id: make_node_id(1),
                        span: make_span(),
                    },
                    id: make_node_id(2),
                    span: make_span(),
                },
                Item::Script {
                    stmt: Stmt::Let {
                        name: make_sym(0), // x (duplicate)
                        mutable: false,
                        ty: None,
                        init: Expr::Literal {
                            value: Literal::Int(2),
                            id: make_node_id(3),
                            span: make_span(),
                        },
                        id: make_node_id(4),
                        span: make_span(),
                    },
                    id: make_node_id(5),
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(
            result.errors[0],
            ResolveError::DuplicateDefinition { .. }
        ));
    }

    #[test]
    fn test_nested_scopes() {
        // let x = 1; { x }
        let ast = ParseResult::new(
            vec![
                Item::Script {
                    stmt: Stmt::Let {
                        name: make_sym(0), // x
                        mutable: false,
                        ty: None,
                        init: Expr::Literal {
                            value: Literal::Int(1),
                            id: make_node_id(0),
                            span: make_span(),
                        },
                        id: make_node_id(1),
                        span: make_span(),
                    },
                    id: make_node_id(2),
                    span: make_span(),
                },
                Item::Script {
                    stmt: Stmt::Expr {
                        expr: Expr::Block {
                            stmts: vec![],
                            expr: Some(Box::new(Expr::Variable {
                                name: make_sym(0), // x
                                id: make_node_id(3),
                                span: make_span(),
                            })),
                            id: make_node_id(4),
                            span: make_span(),
                        },
                    },
                    id: make_node_id(5),
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 0);
        assert!(result.resolutions.contains_key(&make_node_id(3)));
    }

    #[test]
    fn test_block_scope_isolation() {
        // { let x = 1; } x (x not visible outside block)
        let ast = ParseResult::new(
            vec![
                Item::Script {
                    stmt: Stmt::Expr {
                        expr: Expr::Block {
                            stmts: vec![
                                Stmt::Let {
                                    name: make_sym(0), // x
                                    mutable: false,
                                    ty: None,
                                    init: Expr::Literal {
                                        value: Literal::Int(1),
                                        id: make_node_id(0),
                                        span: make_span(),
                                    },
                                    id: make_node_id(1),
                                    span: make_span(),
                                },
                            ],
                            expr: None,
                            id: make_node_id(2),
                            span: make_span(),
                        },
                    },
                    id: make_node_id(3),
                    span: make_span(),
                },
                Item::Script {
                    stmt: Stmt::Expr {
                        expr: Expr::Variable {
                            name: make_sym(0), // x (not visible)
                            id: make_node_id(4),
                            span: make_span(),
                        },
                    },
                    id: make_node_id(5),
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(
            result.errors[0],
            ResolveError::UndefinedVariable { .. }
        ));
    }

    #[test]
    fn test_function_creates_new_scope() {
        // fn foo(x: Int) { let y = x; y }
        let ast = ParseResult::new(
            vec![
                Item::FnDef {
                    id: make_node_id(0),
                    name: make_sym(0), // foo
                    type_params: vec![],
                    params: vec![make_param(make_sym(1), TypeAnnotation::Named(make_sym(2)))],
                    ret_ty: TypeAnnotation::Named(make_sym(2)), // Int
                    body: Expr::Block {
                        stmts: vec![
                            Stmt::Let {
                                name: make_sym(3), // y
                                mutable: false,
                                ty: None,
                                init: Expr::Variable {
                                    name: make_sym(1), // x
                                    id: make_node_id(1),
                                    span: make_span(),
                                },
                                id: make_node_id(2),
                                span: make_span(),
                            },
                        ],
                        expr: Some(Box::new(Expr::Variable {
                            name: make_sym(3), // y
                            id: make_node_id(3),
                            span: make_span(),
                        })),
                        id: make_node_id(4),
                        span: make_span(),
                    },
                    span: make_span(),
                },
            ],
            vec![],
        );

        let result = resolve(&ast);
        assert_eq!(result.errors.len(), 0);
        assert!(result.resolutions.contains_key(&make_node_id(1))); // x
        assert!(result.resolutions.contains_key(&make_node_id(3))); // y
    }
}
