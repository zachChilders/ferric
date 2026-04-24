//! Abstract Syntax Tree types for Ferric.
//!
//! These types represent the parsed structure of Ferric source code.
//! Every node carries a NodeId for later stages to attach metadata,
//! and a Span for error reporting.

use serde::{Deserialize, Serialize};
use crate::{NodeId, Span, Symbol};

/// A named argument at a call site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedArg {
    pub span: Span,
    pub name: Symbol,
    pub value: Box<Expr>,
}

/// A part of a shell command line in the AST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShellPart {
    /// Literal shell text passed through verbatim.
    Literal(String),
    /// The parsed Ferric expression inside `@{...}`.
    Interpolated(Box<Expr>),
}

/// Captured result of running a shell command.
///
/// This is a value type carrying a command's stdout and exit code. Callers
/// must access the fields explicitly — swallowing the exit code is intentionally
/// not the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellOutput {
    pub stdout:    String,
    pub exit_code: i32,
}

// Compile-time assertion: shell types are `Send + Sync` so the async milestone
// can lower `$` expressions to awaitable operations without re-shaping the AST.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ShellOutput>();
    assert_send_sync::<ShellPart>();
};

/// A function parameter definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub span: Span,
    pub name: Symbol,
    pub ty: TypeAnnotation,
    pub default: Option<Box<Expr>>,
}

/// Mode for a require statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RequireMode {
    /// Default — halts on failure
    Error,
    /// Emits a diagnostic warning and continues
    Warn,
}

/// A require statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequireStmt {
    pub span:    Span,
    pub mode:    RequireMode,
    pub expr:    Box<Expr>,
    pub message: Option<Box<Expr>>,
    pub set_fn:  Option<Box<Expr>>,
}

/// Top-level item in a Ferric program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Item {
    /// Function definition
    FnDef {
        /// Unique identifier for this node
        id: NodeId,
        /// Function name
        name: Symbol,
        /// Parameters
        params: Vec<Param>,
        /// Return type
        ret_ty: TypeAnnotation,
        /// Function body
        body: Expr,
        /// Source location
        span: Span,
    },
    /// Top-level script statement (let binding or expression)
    /// These are executed in order when the program runs
    Script {
        /// The statement
        stmt: Stmt,
        /// Unique identifier for this node
        id: NodeId,
        /// Source location
        span: Span,
    },
}

impl Item {
    /// Returns the NodeId of this item.
    pub fn id(&self) -> NodeId {
        match self {
            Item::FnDef { id, .. } => *id,
            Item::Script { id, .. } => *id,
        }
    }

    /// Returns the Span of this item.
    pub fn span(&self) -> Span {
        match self {
            Item::FnDef { span, .. } => *span,
            Item::Script { span, .. } => *span,
        }
    }
}

/// Expression in Ferric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// Literal value (integer, string, boolean, unit)
    Literal {
        value: Literal,
        id: NodeId,
        span: Span,
    },
    /// Variable reference
    Variable {
        name: Symbol,
        id: NodeId,
        span: Span,
    },
    /// Binary operation (e.g., `a + b`)
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Unary operation (e.g., `-x`, `!b`)
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Function call — args are named and in any order at parse time;
    /// resolver canonicalises to definition order before typecheck/VM see it.
    Call {
        callee: Box<Expr>,
        args: Vec<NamedArg>,
        id: NodeId,
        span: Span,
    },
    /// If expression with optional else branch
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        id: NodeId,
        span: Span,
    },
    /// Block expression: `{ stmt*; expr? }`
    Block {
        stmts: Vec<Stmt>,
        expr: Option<Box<Expr>>,
        id: NodeId,
        span: Span,
    },
    /// Return expression
    Return {
        expr: Option<Box<Expr>>,
        id: NodeId,
        span: Span,
    },
    /// While loop: `while cond { body }`
    While {
        cond: Box<Expr>,
        body: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Infinite loop: `loop { body }`
    Loop {
        body: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Break expression
    Break {
        id: NodeId,
        span: Span,
    },
    /// Continue expression
    Continue {
        id: NodeId,
        span: Span,
    },
    /// Closure expression: `|| { body }`
    Closure {
        params: Vec<Param>,
        body: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Shell command expression: `$ cmd @{var} ...`
    Shell {
        parts: Vec<ShellPart>,
        id: NodeId,
        span: Span,
    },
}

impl Expr {
    /// Returns the NodeId of this expression.
    pub fn id(&self) -> NodeId {
        match self {
            Expr::Literal { id, .. } => *id,
            Expr::Variable { id, .. } => *id,
            Expr::Binary { id, .. } => *id,
            Expr::Unary { id, .. } => *id,
            Expr::Call { id, .. } => *id,
            Expr::If { id, .. } => *id,
            Expr::Block { id, .. } => *id,
            Expr::Return { id, .. } => *id,
            Expr::While { id, .. } => *id,
            Expr::Loop { id, .. } => *id,
            Expr::Break { id, .. } => *id,
            Expr::Continue { id, .. } => *id,
            Expr::Closure { id, .. } => *id,
            Expr::Shell { id, .. } => *id,
        }
    }

    /// Returns the Span of this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal { span, .. } => *span,
            Expr::Variable { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::If { span, .. } => *span,
            Expr::Block { span, .. } => *span,
            Expr::Return { span, .. } => *span,
            Expr::While { span, .. } => *span,
            Expr::Loop { span, .. } => *span,
            Expr::Break { span, .. } => *span,
            Expr::Continue { span, .. } => *span,
            Expr::Closure { span, .. } => *span,
            Expr::Shell { span, .. } => *span,
        }
    }
}

/// Statement in Ferric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// Let binding: `let x: Type = expr` or `let mut x: Type = expr`
    Let {
        name: Symbol,
        mutable: bool,
        ty: Option<TypeAnnotation>,
        init: Expr,
        id: NodeId,
        span: Span,
    },
    /// Assignment statement: `x = expr`
    Assign {
        target: Expr,
        value: Expr,
        id: NodeId,
        span: Span,
    },
    /// Expression statement
    Expr {
        expr: Expr,
    },
    /// Require statement
    Require(RequireStmt),
}

impl Stmt {
    /// Returns the NodeId of this statement.
    pub fn id(&self) -> NodeId {
        match self {
            Stmt::Let { id, .. } => *id,
            Stmt::Assign { id, .. } => *id,
            Stmt::Expr { expr } => expr.id(),
            Stmt::Require(req) => req.expr.id(),
        }
    }

    /// Returns the Span of this statement.
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. } => *span,
            Stmt::Assign { span, .. } => *span,
            Stmt::Expr { expr } => expr.span(),
            Stmt::Require(req) => req.span,
        }
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Rem,

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,
}

impl BinOp {
    /// Returns the precedence level of this operator (higher = tighter binding).
    pub fn precedence(&self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne => 3,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 6,
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    /// Numeric negation (`-`)
    Neg,
    /// Logical negation (`!`)
    Not,
}

/// Literal values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// Integer literal
    Int(i64),
    /// Floating-point literal
    Float(f64),
    /// String literal (interned)
    Str(Symbol),
    /// Boolean literal
    Bool(bool),
    /// Unit literal `()`
    Unit,
}

/// Type annotation in source code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeAnnotation {
    /// Named type (e.g., `Int`, `Bool`, `Str`, `Unit`)
    /// In M1, we only support simple named types
    Named(Symbol),
}
