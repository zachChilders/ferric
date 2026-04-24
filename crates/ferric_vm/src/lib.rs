//! # Ferric VM
//!
//! Tree-walking interpreter for Ferric (M1 implementation).
//! This module implements the `Executor` trait with a tree-walking strategy.
//! In M3, a bytecode VM will replace this implementation while maintaining
//! the same `Executor` interface (Rule 6).

use std::collections::HashMap;
use ferric_common::{
    Program, ResolveResult, Symbol, Span, DefId,
    Item, Stmt, Expr, Literal, BinOp, UnOp, Interner, NamedArg,
    RequireStmt, RequireMode, ShellOutput, ShellPart,
};
use ferric_stdlib::{NativeRegistry, NativeValue, NativeFn};

// ============================================================================
// Public API
// ============================================================================

/// Executor trait for running Ferric programs.
///
/// Rule 6: Always use this trait, never call TreeWalker directly.
/// This enables transparent replacement with BytecodeVM in M3.
pub trait Executor {
    /// Executes a program with the given native function registry.
    ///
    /// Returns the result value or a runtime error.
    fn run(&mut self, program: Program, natives: NativeRegistry, interner: &Interner) -> Result<Value, RuntimeError>;
}

/// Tree-walking interpreter implementation.
///
/// Executes the AST directly by recursively evaluating expressions and statements.
pub struct TreeWalker {
    /// Environment stack for local variables (DefId -> Value)
    env_stack: Vec<HashMap<DefId, Value>>,
    /// Symbol to DefId mapping stack for scoping (Symbol -> DefId)
    symbol_stack: Vec<HashMap<Symbol, DefId>>,
    /// Cached AST for function lookups
    ast_items: Vec<Item>,
    /// Cached resolution result for variable lookups
    resolve: ResolveResult,
    /// Native function registry
    natives: NativeRegistry,
    /// String interner for resolving string literals
    interner: Interner,
    /// DefId generator
    next_def_id: u32,
}

impl TreeWalker {
    /// Creates a new tree-walking interpreter.
    pub fn new() -> Self {
        Self {
            env_stack: Vec::new(),
            symbol_stack: Vec::new(),
            ast_items: Vec::new(),
            resolve: ResolveResult::new(HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), Vec::new()),
            natives: NativeRegistry::new(),
            interner: Interner::new(),
            next_def_id: 0,
        }
    }
}

impl Default for TreeWalker {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor for TreeWalker {
    fn run(&mut self, program: Program, natives: NativeRegistry, interner: &Interner) -> Result<Value, RuntimeError> {
        self.natives = natives;
        self.interner = interner.clone();
        self.ast_items = program.items.clone();
        // Use the resolve result carried by the Program (includes canonical_call_args)
        self.resolve = program.resolve;

        // Push global scope
        self.push_env();

        let mut last_value = Value::Unit;

        // Execute all items in order
        for item in &program.items {
            last_value = self.eval_item(item)?;
        }

        // Pop global scope
        self.pop_env();

        Ok(last_value)
    }
}

/// Runtime value types.
///
/// Rule 7: Never construct Value directly outside this module.
/// Use the constructor functions instead (Value::new_int, etc.).
///
/// INVARIANT: `Value` must remain `Send`. Do not add variants containing `Rc`,
/// `RefCell`, raw pointers, or other non-`Send` types. This is required for
/// the async upgrade path — see `ASYNC_COMPAT.md`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    /// M1: Functions stored as DefIds, looked up in AST
    Fn(DefId),
    /// M2.5: Closure value — stores the body expression
    Closure(Box<Expr>),
    /// M2.5: Captured shell output (stdout + exit code).
    ShellOutput(ShellOutput),
}

impl Value {
    // Rule 7 - Value constructor functions (never construct directly!)

    /// Creates an integer value.
    pub fn new_int(n: i64) -> Self {
        Value::Int(n)
    }

    /// Creates a float value.
    pub fn new_float(f: f64) -> Self {
        Value::Float(f)
    }

    /// Creates a boolean value.
    pub fn new_bool(b: bool) -> Self {
        Value::Bool(b)
    }

    /// Creates a string value.
    pub fn new_str(s: String) -> Self {
        Value::Str(s)
    }

    /// Creates a unit value.
    pub fn new_unit() -> Self {
        Value::Unit
    }

    /// Creates a function value (internal use only).
    #[allow(dead_code)]
    pub(crate) fn new_fn(def_id: DefId) -> Self {
        Value::Fn(def_id)
    }

    /// Creates a closure value.
    pub(crate) fn new_closure(body: Expr) -> Self {
        Value::Closure(Box::new(body))
    }

    /// Creates a `ShellOutput` value (Rule 7 — only construction path).
    pub fn new_shell_output(stdout: String, exit_code: i32) -> Self {
        Value::ShellOutput(ShellOutput { stdout, exit_code })
    }
}

/// Runtime errors with source location information.
///
/// Rule 5: All errors must carry a Span.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    UndefinedVariable { name: Symbol, span: Span },
    UndefinedFunction { name: Symbol, span: Span },
    TypeMismatch { expected: String, found: String, span: Span },
    DivisionByZero { span: Span },
    StackOverflow { span: Span },
    NativeFunctionError { message: String, span: Span },
    InvalidOperation { op: String, span: Span },
    NotCallable { span: Span },
    WrongArgumentCount { expected: usize, found: usize, span: Span },
    /// Control flow signals (not actual errors)
    BreakSignal { span: Span },
    ContinueSignal { span: Span },
    /// A require statement with Error mode failed
    RequireError { span: Span, message: Option<String> },
}

// Compile-time assertion: `Value` must be `Send` so a future async runtime can
// carry runtime values across `.await` points. If this fails to compile, a new
// `Value` variant has introduced a non-`Send` type — fix the variant rather
// than weakening the bound. See `ASYNC_COMPAT.md`.
const _: fn() = || {
    fn check<T: Send>() {}
    check::<Value>();
};

// NOTE: The TreeWalker's frame stack is the Rust call stack — `eval_expr`
// recurses into itself, and a function call pushes a Rust frame. This is
// intentional for the M2-era tree-walking interpreter, but it cannot be
// suspended, so it is incompatible with `.await` as-is.
//
// The M3 BytecodeVM replaces this wholesale: frames move to a heap-allocated
// `Vec<Frame>` that can be paused and resumed. That heap stack is the
// primitive an async executor needs. Do not introduce design here that
// assumes the call chain is uninterruptible (e.g. Rust-stack-only data) —
// the M3 replacement will not have that assumption. See `ASYNC_COMPAT.md`.

// ============================================================================
// Private implementation
// ============================================================================

impl TreeWalker {
    /// Pushes a new environment frame.
    fn push_env(&mut self) {
        self.env_stack.push(HashMap::new());
        self.symbol_stack.push(HashMap::new());
    }

    /// Pops the current environment frame.
    fn pop_env(&mut self) {
        self.env_stack.pop();
        self.symbol_stack.pop();
    }

    /// Generates a new DefId.
    fn new_def_id(&mut self) -> DefId {
        let id = DefId::new(self.next_def_id);
        self.next_def_id += 1;
        id
    }

    /// Defines a variable in the current environment.
    fn define(&mut self, def_id: DefId, value: Value) {
        if let Some(env) = self.env_stack.last_mut() {
            env.insert(def_id, value);
        }
    }

    /// Looks up a variable in the environment stack (innermost to outermost).
    fn lookup(&self, def_id: DefId, span: Span) -> Result<Value, RuntimeError> {
        for env in self.env_stack.iter().rev() {
            if let Some(value) = env.get(&def_id) {
                return Ok(value.clone());
            }
        }

        // This shouldn't happen if resolution worked correctly
        Err(RuntimeError::UndefinedVariable {
            name: Symbol::new(0), // We don't have the symbol here
            span,
        })
    }

    /// Evaluates a top-level item.
    fn eval_item(&mut self, item: &Item) -> Result<Value, RuntimeError> {
        match item {
            Item::FnDef { .. } => {
                // Get the DefId for this function from resolution
                // For now, we'll just return Unit - functions are defined globally
                // and don't produce a value when defined
                Ok(Value::Unit)
            }
            Item::Script { stmt, .. } => {
                match stmt {
                    Stmt::Let { .. } | Stmt::Assign { .. } | Stmt::Require(_) => {
                        self.eval_stmt(stmt)?;
                        Ok(Value::Unit)
                    }
                    Stmt::Expr { expr } => {
                        // Expression statements return their value
                        self.eval_expr(expr)
                    }
                }
            }
        }
    }

    /// Evaluates a statement.
    fn eval_stmt(&mut self, stmt: &Stmt) -> Result<(), RuntimeError> {
        match stmt {
            Stmt::Let { name, init, .. } => {
                // Evaluate the initializer
                let value = self.eval_expr(init)?;

                // Create a new DefId for this binding
                let def_id = self.new_def_id();

                // Store the symbol -> DefId mapping
                if let Some(scope) = self.symbol_stack.last_mut() {
                    scope.insert(*name, def_id);
                }

                // Define the variable
                self.define(def_id, value);
                Ok(())
            }
            Stmt::Assign { target, value, span, .. } => {
                // Evaluate the value
                let val = self.eval_expr(value)?;

                // For M2, we only support simple variable assignment
                if let Expr::Variable { name, .. } = target {
                    // Look up the DefId for this variable
                    for scope in self.symbol_stack.iter().rev() {
                        if let Some(def_id) = scope.get(name) {
                            // Update the value in the environment
                            // Search through env_stack to find and update this DefId
                            for env in self.env_stack.iter_mut().rev() {
                                if env.contains_key(def_id) {
                                    env.insert(*def_id, val);
                                    return Ok(());
                                }
                            }
                        }
                    }
                    // Variable not found - this should have been caught by resolver
                    return Err(RuntimeError::UndefinedVariable {
                        name: *name,
                        span: *span,
                    });
                } else {
                    // For M2, we only support variable assignment
                    return Err(RuntimeError::InvalidOperation {
                        op: "assignment to non-variable".to_string(),
                        span: *span,
                    });
                }
            }
            Stmt::Expr { expr } => {
                self.eval_expr(expr)?;
                Ok(())
            }
            Stmt::Require(req) => {
                self.eval_require(req)
            }
        }
    }

    /// Evaluates a require statement.
    fn eval_require(&mut self, req: &RequireStmt) -> Result<(), RuntimeError> {
        // 1. Evaluate condition
        let mut cond = self.eval_expr(&req.expr)?;

        // 2. If false and set_fn present, call it and retry once
        if matches!(cond, Value::Bool(false)) {
            if let Some(set_fn_expr) = &req.set_fn {
                let closure_val = self.eval_expr(set_fn_expr)?;
                if let Value::Closure(body) = closure_val {
                    self.eval_expr(&body)?; // discard return value
                }
                // Re-evaluate condition once
                cond = self.eval_expr(&req.expr)?;
            }
        }

        // 3. If still false, apply mode
        if matches!(cond, Value::Bool(false)) {
            // Only evaluate message on failure (lazy)
            let message = if let Some(msg_expr) = &req.message {
                match self.eval_expr(msg_expr)? {
                    Value::Str(s) => Some(s),
                    _ => None,
                }
            } else {
                None
            };

            match req.mode {
                RequireMode::Error => {
                    return Err(RuntimeError::RequireError { span: req.span, message });
                }
                RequireMode::Warn => {
                    let msg_str = message.unwrap_or_else(|| "require condition evaluated to false".to_string());
                    eprintln!("warning: require failed: {}", msg_str);
                }
            }
        }

        Ok(())
    }

    /// Evaluates an expression.
    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::Literal { value, .. } => {
                self.eval_literal(value)
            }
            Expr::Variable { name, span, .. } => {
                // Look up the symbol in our symbol stack (innermost to outermost)
                for scope in self.symbol_stack.iter().rev() {
                    if let Some(def_id) = scope.get(name) {
                        return self.lookup(*def_id, *span);
                    }
                }
                Err(RuntimeError::UndefinedVariable {
                    name: *name,
                    span: *span,
                })
            }
            Expr::Binary { op, left, right, span, .. } => {
                let left_val = self.eval_expr(left)?;
                let right_val = self.eval_expr(right)?;
                self.eval_binop(*op, &left_val, &right_val, *span)
            }
            Expr::Unary { op, expr, span, .. } => {
                let val = self.eval_expr(expr)?;
                self.eval_unop(*op, &val, *span)
            }
            Expr::Call { callee, args, span, id } => {
                // Use canonical args (definition order, defaults inserted) when available
                let canonical = self.resolve.canonical_call_args.get(id).cloned();
                let args_ref: &[NamedArg] = canonical.as_deref().unwrap_or(args);
                self.eval_call(callee, args_ref, *span)
            }
            Expr::If { cond, then_branch, else_branch, span, .. } => {
                let cond_val = self.eval_expr(cond)?;
                match cond_val {
                    Value::Bool(true) => self.eval_expr(then_branch),
                    Value::Bool(false) => {
                        if let Some(else_expr) = else_branch {
                            self.eval_expr(else_expr)
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    _ => Err(RuntimeError::TypeMismatch {
                        expected: "Bool".to_string(),
                        found: self.type_name(&cond_val),
                        span: *span,
                    }),
                }
            }
            Expr::Block { stmts, expr, .. } => {
                self.push_env();

                for stmt in stmts {
                    self.eval_stmt(stmt)?;
                }

                let result = if let Some(e) = expr {
                    self.eval_expr(e)?
                } else {
                    Value::Unit
                };

                self.pop_env();
                Ok(result)
            }
            Expr::Return { expr, .. } => {
                let value = if let Some(e) = expr {
                    self.eval_expr(e)?
                } else {
                    Value::Unit
                };

                // For now, we'll just return the value
                // In a more complete implementation, we'd use Result with ControlFlow
                Ok(value)
            }

            Expr::While { cond, body, span, .. } => {
                loop {
                    // Evaluate condition
                    let cond_val = self.eval_expr(cond)?;

                    match cond_val {
                        Value::Bool(true) => {
                            // Execute body, handling break/continue
                            match self.eval_expr(body) {
                                Ok(_) => continue, // Continue to next iteration
                                Err(RuntimeError::BreakSignal { .. }) => break, // Exit loop
                                Err(RuntimeError::ContinueSignal { .. }) => continue, // Next iteration
                                Err(e) => return Err(e), // Propagate other errors
                            }
                        }
                        Value::Bool(false) => break, // Exit loop
                        _ => return Err(RuntimeError::TypeMismatch {
                            expected: "Bool".to_string(),
                            found: self.type_name(&cond_val),
                            span: *span,
                        }),
                    }
                }
                Ok(Value::Unit)
            }

            Expr::Loop { body, .. } => {
                loop {
                    // Execute body, handling break/continue
                    match self.eval_expr(body) {
                        Ok(_) => continue, // Continue to next iteration
                        Err(RuntimeError::BreakSignal { .. }) => break, // Exit loop
                        Err(RuntimeError::ContinueSignal { .. }) => continue, // Next iteration
                        Err(e) => return Err(e), // Propagate other errors
                    }
                }
                Ok(Value::Unit)
            }

            Expr::Break { span, .. } => {
                // Signal a break by returning a "BreakSignal" error
                Err(RuntimeError::BreakSignal { span: *span })
            }

            Expr::Continue { span, .. } => {
                // Signal a continue by returning a "ContinueSignal" error
                Err(RuntimeError::ContinueSignal { span: *span })
            }

            Expr::Closure { body, .. } => {
                // A closure evaluates to a Value::Closure carrying its body
                Ok(Value::new_closure(*body.clone()))
            }

            Expr::Shell { parts, span, .. } => self.eval_shell(parts, *span),
        }
    }

    /// Builds the command string from shell parts and runs it as a subprocess.
    fn eval_shell(&mut self, parts: &[ShellPart], span: Span) -> Result<Value, RuntimeError> {
        let mut cmd = String::new();
        for part in parts {
            match part {
                ShellPart::Literal(s) => cmd.push_str(s),
                ShellPart::Interpolated(expr) => {
                    let v = self.eval_expr(expr)?;
                    match v {
                        Value::Str(s) => cmd.push_str(&s),
                        Value::Int(n) => cmd.push_str(&n.to_string()),
                        other => {
                            // Type checker should have rejected these; treat as runtime mismatch.
                            return Err(RuntimeError::TypeMismatch {
                                expected: "Str or Int".to_string(),
                                found: self.type_name(&other),
                                span,
                            });
                        }
                    }
                }
            }
        }
        Ok(run_shell_command(&cmd))
    }

    /// Evaluates a literal.
    fn eval_literal(&self, lit: &Literal) -> Result<Value, RuntimeError> {
        match lit {
            Literal::Int(n) => Ok(Value::Int(*n)),
            Literal::Float(f) => Ok(Value::Float(*f)),
            Literal::Bool(b) => Ok(Value::Bool(*b)),
            Literal::Str(sym) => {
                // Resolve the string from the interner
                let s = self.interner.resolve(*sym);
                Ok(Value::Str(s.to_string()))
            }
            Literal::Unit => Ok(Value::Unit),
        }
    }

    /// Evaluates a binary operation.
    fn eval_binop(&self, op: BinOp, left: &Value, right: &Value, span: Span) -> Result<Value, RuntimeError> {
        match op {
            BinOp::Add => left.add(right, span),
            BinOp::Sub => left.sub(right, span),
            BinOp::Mul => left.mul(right, span),
            BinOp::Div => left.div(right, span),
            BinOp::Rem => left.rem(right, span),
            BinOp::Eq => left.eq_op(right, span),
            BinOp::Ne => {
                let eq = left.eq_op(right, span)?;
                match eq {
                    Value::Bool(b) => Ok(Value::Bool(!b)),
                    _ => unreachable!(),
                }
            }
            BinOp::Lt => left.lt(right, span),
            BinOp::Le => left.le(right, span),
            BinOp::Gt => left.gt(right, span),
            BinOp::Ge => left.ge(right, span),
            BinOp::And => {
                match (left, right) {
                    (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
                    _ => Err(RuntimeError::TypeMismatch {
                        expected: "Bool".to_string(),
                        found: self.type_name(left),
                        span,
                    }),
                }
            }
            BinOp::Or => {
                match (left, right) {
                    (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
                    _ => Err(RuntimeError::TypeMismatch {
                        expected: "Bool".to_string(),
                        found: self.type_name(left),
                        span,
                    }),
                }
            }
        }
    }

    /// Evaluates a unary operation.
    fn eval_unop(&self, op: UnOp, val: &Value, span: Span) -> Result<Value, RuntimeError> {
        match op {
            UnOp::Neg => {
                match val {
                    Value::Int(n) => Ok(Value::Int(-n)),
                    Value::Float(f) => Ok(Value::Float(-f)),
                    _ => Err(RuntimeError::TypeMismatch {
                        expected: "Int or Float".to_string(),
                        found: self.type_name(val),
                        span,
                    }),
                }
            }
            UnOp::Not => {
                match val {
                    Value::Bool(b) => Ok(Value::Bool(!b)),
                    _ => Err(RuntimeError::TypeMismatch {
                        expected: "Bool".to_string(),
                        found: self.type_name(val),
                        span,
                    }),
                }
            }
        }
    }

    /// Evaluates a function call.
    fn eval_call(&mut self, callee: &Expr, args: &[NamedArg], span: Span) -> Result<Value, RuntimeError> {
        // For M1, we only support direct function name calls
        if let Expr::Variable { name, .. } = callee {
            // Check if it's a native function
            if let Some(native_fn) = self.natives.get(*name) {
                return self.call_native(*native_fn, args, span);
            }

            // Otherwise, look for a user-defined function
            return self.call_user_function(*name, args, span);
        }

        Err(RuntimeError::NotCallable { span })
    }

    /// Calls a native function.
    fn call_native(&mut self, native_fn: NativeFn, args: &[NamedArg], span: Span) -> Result<Value, RuntimeError> {
        // Evaluate all arguments (already in definition order from resolver)
        let mut arg_values = Vec::new();
        for arg in args {
            arg_values.push(self.eval_expr(&arg.value)?);
        }

        // Convert to NativeValue
        let native_args: Vec<NativeValue> = arg_values.iter()
            .map(|v| self.value_to_native(v))
            .collect();

        // Call the native function
        match native_fn(&native_args) {
            Ok(native_result) => Ok(self.native_to_value(&native_result)),
            Err(msg) => Err(RuntimeError::NativeFunctionError {
                message: msg,
                span,
            }),
        }
    }

    /// Calls a user-defined function.
    fn call_user_function(&mut self, name: Symbol, args: &[NamedArg], span: Span) -> Result<Value, RuntimeError> {
        // Find the function definition in the AST and clone it to avoid borrow issues
        let fn_def = self.ast_items.iter().find_map(|item| {
            if let Item::FnDef { name: fn_name, params, body, .. } = item {
                if *fn_name == name {
                    return Some((params.clone(), body.clone()));
                }
            }
            None
        });

        let (params, body) = fn_def.ok_or(RuntimeError::UndefinedFunction { name, span })?;

        // Check argument count (canonical args always match after resolver validation)
        if args.len() != params.len() {
            return Err(RuntimeError::WrongArgumentCount {
                expected: params.len(),
                found: args.len(),
                span,
            });
        }

        // Evaluate all arguments (already in definition order from resolver)
        let mut arg_values = Vec::new();
        for arg in args {
            arg_values.push(self.eval_expr(&arg.value)?);
        }

        // Push a new environment for the function
        self.push_env();

        // Bind parameters to arguments (params[i] ↔ args[i] by definition order)
        for (i, param) in params.iter().enumerate() {
            // Create a new DefId for the parameter
            let def_id = self.new_def_id();

            // Store the symbol -> DefId mapping
            if let Some(scope) = self.symbol_stack.last_mut() {
                scope.insert(param.name, def_id);
            }

            self.define(def_id, arg_values[i].clone());
        }

        // Execute the function body
        let result = self.eval_expr(&body)?;

        // Pop the function environment
        self.pop_env();

        Ok(result)
    }

    /// Converts a Value to NativeValue.
    fn value_to_native(&self, value: &Value) -> NativeValue {
        match value {
            Value::Int(n) => NativeValue::Int(*n),
            Value::Float(f) => NativeValue::Float(*f),
            Value::Bool(b) => NativeValue::Bool(*b),
            Value::Str(s) => NativeValue::Str(s.clone()),
            Value::Unit => NativeValue::Unit,
            Value::Fn(_) => NativeValue::Unit,
            Value::Closure(_) => NativeValue::Unit,
            Value::ShellOutput(out) => NativeValue::ShellOutput(out.clone()),
        }
    }

    /// Converts a NativeValue to Value.
    fn native_to_value(&self, native: &NativeValue) -> Value {
        match native {
            NativeValue::Int(n) => Value::Int(*n),
            NativeValue::Float(f) => Value::Float(*f),
            NativeValue::Bool(b) => Value::Bool(*b),
            NativeValue::Str(s) => Value::Str(s.clone()),
            NativeValue::Unit => Value::Unit,
            NativeValue::ShellOutput(out) => Value::ShellOutput(out.clone()),
        }
    }

    /// Returns a string representation of a value's type.
    fn type_name(&self, value: &Value) -> String {
        match value {
            Value::Int(_) => "Int".to_string(),
            Value::Float(_) => "Float".to_string(),
            Value::Bool(_) => "Bool".to_string(),
            Value::Str(_) => "Str".to_string(),
            Value::Unit => "Unit".to_string(),
            Value::Fn(_) => "Fn".to_string(),
            Value::Closure(_) => "Fn".to_string(),
            Value::ShellOutput(_) => "ShellOutput".to_string(),
        }
    }
}

/// Runs a shell command synchronously and returns a `Value::ShellOutput`.
///
/// On Unix this delegates to `sh -c <cmd>`; on Windows to `cmd /C <cmd>`.
/// On targets without subprocess support (e.g. WASM), returns exit code 126
/// (the conventional "command not executable" code).
fn run_shell_command(cmd: &str) -> Value {
    #[cfg(any(unix, windows))]
    {
        let output = if cfg!(windows) {
            std::process::Command::new("cmd").arg("/C").arg(cmd).output()
        } else {
            std::process::Command::new("sh").arg("-c").arg(cmd).output()
        };
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                let exit_code = out.status.code().unwrap_or(-1);
                Value::new_shell_output(stdout, exit_code)
            }
            Err(_) => Value::new_shell_output(String::new(), 126),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
        Value::new_shell_output(String::new(), 126)
    }
}

/// Value operations implementation.
impl Value {
    /// Addition operation.
    pub fn add(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric or string types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Subtraction operation.
    pub fn sub(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Multiplication operation.
    pub fn mul(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Division operation.
    pub fn div(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(RuntimeError::DivisionByZero { span })
                } else {
                    Ok(Value::Int(a / b))
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    Err(RuntimeError::DivisionByZero { span })
                } else {
                    Ok(Value::Float(a / b))
                }
            }
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Remainder operation.
    pub fn rem(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(RuntimeError::DivisionByZero { span })
                } else {
                    Ok(Value::Int(a % b))
                }
            }
            _ => Err(RuntimeError::TypeMismatch {
                expected: "Int".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Equality operation.
    pub fn eq_op(&self, other: &Value, _span: Span) -> Result<Value, RuntimeError> {
        Ok(Value::Bool(self == other))
    }

    /// Less-than operation.
    pub fn lt(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Less-than-or-equal operation.
    pub fn le(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Greater-than operation.
    pub fn gt(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }

    /// Greater-than-or-equal operation.
    pub fn ge(&self, other: &Value, span: Span) -> Result<Value, RuntimeError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "matching numeric types".to_string(),
                found: format!("{:?} and {:?}", self, other),
                span,
            }),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::NodeId;

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
    fn test_integer_arithmetic() {
        // 1 + 2 * 3 = 7
        let items = vec![
            Item::Script {
                stmt: Stmt::Expr {
                    expr: Expr::Binary {
                        op: BinOp::Add,
                        left: Box::new(Expr::Literal {
                            value: Literal::Int(1),
                            id: make_node_id(0),
                            span: make_span(),
                        }),
                        right: Box::new(Expr::Binary {
                            op: BinOp::Mul,
                            left: Box::new(Expr::Literal {
                                value: Literal::Int(2),
                                id: make_node_id(1),
                                span: make_span(),
                            }),
                            right: Box::new(Expr::Literal {
                                value: Literal::Int(3),
                                id: make_node_id(2),
                                span: make_span(),
                            }),
                            id: make_node_id(3),
                            span: make_span(),
                        }),
                        id: make_node_id(4),
                        span: make_span(),
                    },
                },
                id: make_node_id(5),
                span: make_span(),
            },
        ];

        let program = Program::new(items, ResolveResult::new(HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), vec![]));
        let mut vm = TreeWalker::new();
        let natives = NativeRegistry::new();
        let interner = Interner::new();

        let result = vm.run(program, natives, &interner).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn test_string_concatenation() {
        // "hello" + " " + "world"
        // First, set up an interner with the strings
        let mut interner = Interner::new();
        let hello_sym = interner.intern("hello");
        let space_sym = interner.intern(" ");
        let world_sym = interner.intern("world");

        let items = vec![
            Item::Script {
                stmt: Stmt::Expr {
                    expr: Expr::Binary {
                        op: BinOp::Add,
                        left: Box::new(Expr::Binary {
                            op: BinOp::Add,
                            left: Box::new(Expr::Literal {
                                value: Literal::Str(hello_sym),
                                id: make_node_id(0),
                                span: make_span(),
                            }),
                            right: Box::new(Expr::Literal {
                                value: Literal::Str(space_sym),
                                id: make_node_id(1),
                                span: make_span(),
                            }),
                            id: make_node_id(2),
                            span: make_span(),
                        }),
                        right: Box::new(Expr::Literal {
                            value: Literal::Str(world_sym),
                            id: make_node_id(3),
                            span: make_span(),
                        }),
                        id: make_node_id(4),
                        span: make_span(),
                    },
                },
                id: make_node_id(5),
                span: make_span(),
            },
        ];

        let program = Program::new(items, ResolveResult::new(HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), vec![]));
        let mut vm = TreeWalker::new();
        let natives = NativeRegistry::new();

        let result = vm.run(program, natives, &interner).unwrap();
        assert_eq!(result, Value::Str("hello world".to_string()));
    }

    #[test]
    fn test_boolean_logic() {
        // true && false = false
        let items = vec![
            Item::Script {
                stmt: Stmt::Expr {
                    expr: Expr::Binary {
                        op: BinOp::And,
                        left: Box::new(Expr::Literal {
                            value: Literal::Bool(true),
                            id: make_node_id(0),
                            span: make_span(),
                        }),
                        right: Box::new(Expr::Literal {
                            value: Literal::Bool(false),
                            id: make_node_id(1),
                            span: make_span(),
                        }),
                        id: make_node_id(2),
                        span: make_span(),
                    },
                },
                id: make_node_id(3),
                span: make_span(),
            },
        ];

        let program = Program::new(items, ResolveResult::new(HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), vec![]));
        let mut vm = TreeWalker::new();
        let natives = NativeRegistry::new();
        let interner = Interner::new();

        let result = vm.run(program, natives, &interner).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn test_if_expression() {
        // if true { 1 } else { 2 } = 1
        let items = vec![
            Item::Script {
                stmt: Stmt::Expr {
                    expr: Expr::If {
                        cond: Box::new(Expr::Literal {
                            value: Literal::Bool(true),
                            id: make_node_id(0),
                            span: make_span(),
                        }),
                        then_branch: Box::new(Expr::Literal {
                            value: Literal::Int(1),
                            id: make_node_id(1),
                            span: make_span(),
                        }),
                        else_branch: Some(Box::new(Expr::Literal {
                            value: Literal::Int(2),
                            id: make_node_id(2),
                            span: make_span(),
                        })),
                        id: make_node_id(3),
                        span: make_span(),
                    },
                },
                id: make_node_id(4),
                span: make_span(),
            },
        ];

        let program = Program::new(items, ResolveResult::new(HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), vec![]));
        let mut vm = TreeWalker::new();
        let natives = NativeRegistry::new();
        let interner = Interner::new();

        let result = vm.run(program, natives, &interner).unwrap();
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn test_division_by_zero() {
        // 5 / 0
        let items = vec![
            Item::Script {
                stmt: Stmt::Expr {
                    expr: Expr::Binary {
                        op: BinOp::Div,
                        left: Box::new(Expr::Literal {
                            value: Literal::Int(5),
                            id: make_node_id(0),
                            span: make_span(),
                        }),
                        right: Box::new(Expr::Literal {
                            value: Literal::Int(0),
                            id: make_node_id(1),
                            span: make_span(),
                        }),
                        id: make_node_id(2),
                        span: make_span(),
                    },
                },
                id: make_node_id(3),
                span: make_span(),
            },
        ];

        let program = Program::new(items, ResolveResult::new(HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), vec![]));
        let mut vm = TreeWalker::new();
        let natives = NativeRegistry::new();
        let interner = Interner::new();

        let result = vm.run(program, natives, &interner);
        assert!(matches!(result, Err(RuntimeError::DivisionByZero { .. })));
    }

    #[test]
    fn test_value_constructors() {
        // Test Rule 7: Value constructor functions
        let int_val = Value::new_int(42);
        assert_eq!(int_val, Value::Int(42));

        let float_val = Value::new_float(3.14);
        assert_eq!(float_val, Value::Float(3.14));

        let bool_val = Value::new_bool(true);
        assert_eq!(bool_val, Value::Bool(true));

        let str_val = Value::new_str("hello".to_string());
        assert_eq!(str_val, Value::Str("hello".to_string()));

        let unit_val = Value::new_unit();
        assert_eq!(unit_val, Value::Unit);
    }

    #[test]
    fn test_runtime_errors_have_spans() {
        // Test Rule 5: All errors must carry a Span
        let span = Span::new(10, 20);

        let err1 = RuntimeError::DivisionByZero { span };
        match err1 {
            RuntimeError::DivisionByZero { span: s } => assert_eq!(s, span),
            _ => panic!("Wrong error type"),
        }

        let err2 = RuntimeError::TypeMismatch {
            expected: "Int".to_string(),
            found: "Bool".to_string(),
            span,
        };
        match err2 {
            RuntimeError::TypeMismatch { span: s, .. } => assert_eq!(s, span),
            _ => panic!("Wrong error type"),
        }
    }
}
