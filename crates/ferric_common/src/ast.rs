//! Abstract Syntax Tree types for Ferric.
//!
//! These types represent the parsed structure of Ferric source code.
//! Every node carries a NodeId for later stages to attach metadata,
//! and a Span for error reporting.

use crate::{NodeId, Span, Symbol};

/// Top-level item in a Ferric program.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Function definition
    FnDef {
        /// Unique identifier for this node
        id: NodeId,
        /// Function name
        name: Symbol,
        /// Parameters: (name, type)
        params: Vec<(Symbol, TypeAnnotation)>,
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
#[derive(Debug, Clone, PartialEq)]
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
    /// Function call (e.g., `foo(1, 2)`)
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
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
        }
    }
}

/// Statement in Ferric.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Let binding: `let x: Type = expr`
    Let {
        name: Symbol,
        ty: Option<TypeAnnotation>,
        init: Expr,
        id: NodeId,
        span: Span,
    },
    /// Expression statement
    Expr {
        expr: Expr,
    },
}

impl Stmt {
    /// Returns the NodeId of this statement.
    pub fn id(&self) -> NodeId {
        match self {
            Stmt::Let { id, .. } => *id,
            Stmt::Expr { expr } => expr.id(),
        }
    }

    /// Returns the Span of this statement.
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. } => *span,
            Stmt::Expr { expr } => expr.span(),
        }
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// Numeric negation (`-`)
    Neg,
    /// Logical negation (`!`)
    Not,
}

/// Literal values.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// Integer literal
    Int(i64),
    /// String literal (interned)
    Str(Symbol),
    /// Boolean literal
    Bool(bool),
    /// Unit literal `()`
    Unit,
}

/// Type annotation in source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeAnnotation {
    /// Named type (e.g., `Int`, `Bool`, `Str`, `Unit`)
    /// In M1, we only support simple named types
    Named(Symbol),
}
