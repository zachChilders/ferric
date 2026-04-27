# Task: M1 Parser Implementation

## Objective
Implement the `ferric_parser` crate with a recursive descent parser that produces an AST for Milestone 1 features.

## Architecture Context
- The parser is the second stage in the pipeline
- It must expose exactly one public function: `pub fn parse(lex: &LexResult) -> ParseResult`
- All internal helpers and AST traversal logic must be private
- The parser consumes `LexResult` and produces `ParseResult` (both from `ferric_common`)

## Public Interface (Non-Negotiable)

```rust
// ferric_parser/src/lib.rs
pub fn parse(lex: &LexResult) -> ParseResult;
```

## Feature Requirements

### AST Node Types (defined in ferric_common)

```rust
pub enum Item {
    FnDef {
        id: NodeId,
        name: Symbol,
        params: Vec<(Symbol, TypeAnnotation)>,
        ret_ty: TypeAnnotation,
        body: Expr,
        span: Span,
    },
}

pub enum Expr {
    Literal { value: Literal, id: NodeId, span: Span },
    Variable { name: Symbol, id: NodeId, span: Span },
    Binary { op: BinOp, left: Box<Expr>, right: Box<Expr>, id: NodeId, span: Span },
    Unary { op: UnOp, expr: Box<Expr>, id: NodeId, span: Span },
    Call { callee: Box<Expr>, args: Vec<Expr>, id: NodeId, span: Span },
    If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Option<Box<Expr>>, id: NodeId, span: Span },
    Block { stmts: Vec<Stmt>, expr: Option<Box<Expr>>, id: NodeId, span: Span },
    Return { expr: Option<Box<Expr>>, id: NodeId, span: Span },
}

pub enum Stmt {
    Let { name: Symbol, ty: Option<TypeAnnotation>, init: Expr, id: NodeId, span: Span },
    Expr { expr: Expr },
}

pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

pub enum UnOp {
    Neg, Not,
}

pub enum Literal {
    Int(i64),
    Str(Symbol),
    Bool(bool),
    Unit,
}

pub enum TypeAnnotation {
    Named(Symbol),  // M1: just "Int", "Str", "Bool", "Unit"
}
```

### Grammar to Implement (M1)

```
program    := item*
item       := fn_def

fn_def     := "fn" IDENT "(" params? ")" ret_type? block
params     := param ("," param)*
param      := IDENT ":" type
ret_type   := "->" type
type       := IDENT  // M1: just named types (Int, Str, Bool, Unit)

block      := "{" stmt* expr? "}"
stmt       := let_stmt | expr_stmt
let_stmt   := "let" IDENT (":" type)? "=" expr ";"?
expr_stmt  := expr ";"?

expr       := return_expr | if_expr | binary_expr
return_expr := "return" expr?
if_expr    := "if" expr block ("else" (if_expr | block))?

binary_expr := unary_expr (bin_op unary_expr)*
unary_expr  := un_op unary_expr | call_expr
call_expr   := primary ("(" args? ")")*
primary     := literal | variable | "(" expr ")" | block

args       := expr ("," expr)*
literal    := INT_LIT | STR_LIT | "true" | "false"
variable   := IDENT

bin_op     := "+" | "-" | "*" | "/" | "%" | "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||"
un_op      := "-" | "!"
```

### Operator Precedence (lowest to highest)
1. `||` (logical or)
2. `&&` (logical and)
3. `==`, `!=` (equality)
4. `<`, `<=`, `>`, `>=` (comparison)
5. `+`, `-` (addition)
6. `*`, `/`, `%` (multiplication)
7. unary `-`, `!` (negation)
8. function call

### NodeId Assignment
Every expression and statement must get a unique `NodeId`.
Use a simple counter that increments for each node created.

```rust
struct NodeIdGen {
    next: u32,
}

impl NodeIdGen {
    fn new() -> Self { Self { next: 0 } }
    fn next(&mut self) -> NodeId {
        let id = NodeId(self.next);
        self.next += 1;
        id
    }
}
```

## Error Handling (Rule 5)

Never panic. Use error recovery to continue parsing after an error.

```rust
pub enum ParseError {
    UnexpectedToken { expected: String, found: TokenKind, span: Span },
    UnexpectedEof { expected: String, span: Span },
}
```

### Error Recovery Strategies
- If a semicolon is missing, insert one mentally and continue
- If a closing delimiter is missing, try to skip to a likely recovery point
- Track brace/paren depth to avoid cascading errors
- Accumulate errors and keep parsing - don't give up after the first error

## Implementation Notes

### Parser Structure
```rust
struct Parser<'a> {
    tokens: &'a [Token],
    current: usize,
    node_id_gen: NodeIdGen,
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self;

    // Traversal
    fn peek(&self) -> &Token;
    fn advance(&mut self) -> &Token;
    fn check(&self, kind: &TokenKind) -> bool;
    fn expect(&mut self, kind: TokenKind, msg: &str) -> Result<&Token, ()>;

    // Parsing methods
    fn parse_program(&mut self) -> Vec<Item>;
    fn parse_fn_def(&mut self) -> Option<Item>;
    fn parse_block(&mut self) -> Expr;
    fn parse_stmt(&mut self) -> Option<Stmt>;
    fn parse_expr(&mut self) -> Expr;
    fn parse_binary_expr(&mut self, min_prec: u8) -> Expr;
    fn parse_unary_expr(&mut self) -> Expr;
    fn parse_call_expr(&mut self) -> Expr;
    fn parse_primary(&mut self) -> Expr;
    fn parse_if_expr(&mut self) -> Expr;
    fn parse_return_expr(&mut self) -> Expr;
    fn parse_type(&mut self) -> TypeAnnotation;
}
```

### Pratt Parsing for Binary Expressions
Use a precedence climbing algorithm for clean operator handling:
```rust
fn precedence(op: &BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne => 3,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
        BinOp::Add | BinOp::Sub => 5,
        BinOp::Mul | BinOp::Div | BinOp::Rem => 6,
    }
}
```

## Test Cases

Create unit tests for:
1. Empty program
2. Simple function: `fn foo() { }`
3. Function with params: `fn add(x: Int, y: Int) -> Int { x + y }`
4. Let binding: `let x = 5`
5. Let binding with type: `let x: Int = 5`
6. Binary operators with correct precedence: `1 + 2 * 3` parses as `1 + (2 * 3)`
7. If expression: `if x { y } else { z }`
8. Block expression: `{ let x = 5; x + 1 }`
9. Function call: `foo(1, 2, 3)`
10. Return statement: `return x + 1`
11. Error recovery: missing semicolon doesn't crash parser

## Acceptance Criteria
- [ ] Public API is exactly `pub fn parse(lex: &LexResult) -> ParseResult`
- [ ] All M1 grammar features parse correctly
- [ ] Operator precedence is correct
- [ ] Every AST node has a unique NodeId
- [ ] Every AST node has an accurate Span
- [ ] All errors carry Spans
- [ ] Parser never panics - errors are accumulated
- [ ] Parser continues after errors (error recovery)
- [ ] All unit tests pass
- [ ] No public exports other than `parse` function

## Critical Rules to Enforce

### Rule 1 - Stages communicate only through output types
The parser depends on `ferric_common` types only, not on `ferric_lexer` internals.

### Rule 2 - Exactly one public entry point
Only `parse()` should be public.

### Rule 5 - Every error carries a Span
Every `ParseError` variant must include a `Span`.

## Notes for Agent
- Use recursive descent parsing - it's simple and sufficient for M1
- Keep error messages clear and actionable
- Span tracking is critical - test it thoroughly
- Don't worry about performance - focus on correctness
- Make sure every node gets a NodeId - this will be used by later stages
