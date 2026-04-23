//! # Ferric Parser
//!
//! The parser stage takes tokens from the lexer and builds an Abstract Syntax Tree (AST).
//! It implements a recursive descent parser with operator precedence climbing for
//! binary expressions.
//!
//! ## Public API
//!
//! This crate exposes exactly one public function:
//! - `parse(&LexResult) -> ParseResult`
//!
//! All other implementation details are private.

use ferric_common::{
    BinOp, Expr, Item, LexResult, Literal, NodeId, ParseError, ParseResult, Stmt, Symbol,
    Token, TokenKind, TypeAnnotation, UnOp,
};

/// Generates unique NodeIds for AST nodes.
struct NodeIdGen {
    next: u32,
}

impl NodeIdGen {
    /// Creates a new NodeIdGen starting from 0.
    fn new() -> Self {
        Self { next: 0 }
    }

    /// Returns the next NodeId and increments the counter.
    fn next(&mut self) -> NodeId {
        let id = NodeId::new(self.next);
        self.next += 1;
        id
    }
}

/// Recursive descent parser for Ferric.
struct Parser<'a> {
    /// Input tokens
    tokens: &'a [Token],
    /// Current position in token stream
    current: usize,
    /// NodeId generator
    node_id_gen: NodeIdGen,
    /// Accumulated errors
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    /// Creates a new parser for the given tokens.
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            current: 0,
            node_id_gen: NodeIdGen::new(),
            errors: Vec::new(),
        }
    }

    // ========== Token Traversal ==========

    /// Returns the current token without consuming it.
    fn peek(&self) -> &Token {
        self.tokens.get(self.current).unwrap_or_else(|| {
            // If we're past the end, return the last token (should be EOF)
            self.tokens.last().expect("Token stream should not be empty")
        })
    }

    /// Advances to the next token and returns the previous current token.
    fn advance(&mut self) -> &Token {
        let current = self.current;
        if self.current < self.tokens.len() {
            self.current += 1;
        }
        self.tokens.get(current).unwrap_or_else(|| {
            self.tokens.last().expect("Token stream should not be empty")
        })
    }

    /// Returns true if the current token matches the given kind.
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    /// Returns true if we're at the end of the token stream.
    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    /// Consumes the current token if it matches the expected kind.
    /// Returns an error and attempts recovery if it doesn't match.
    fn expect(&mut self, kind: TokenKind, expected: &str) -> Result<Token, ()> {
        if self.check(&kind) {
            Ok(self.advance().clone())
        } else {
            let found = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: expected.to_string(),
                found: found.kind.clone(),
                span: found.span,
            });
            Err(())
        }
    }

    /// Consumes the current token if it matches the expected kind, but doesn't error if not.
    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    // ========== Parsing Methods ==========

    /// Parses a complete program (list of top-level items).
    fn parse_program(&mut self) -> Vec<Item> {
        let mut items = Vec::new();

        while !self.is_at_end() {
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                // Skip to next likely item start on error
                self.synchronize();
            }
        }

        items
    }

    /// Attempts to synchronize after a parse error by advancing to a likely recovery point.
    fn synchronize(&mut self) {
        self.advance();

        while !self.is_at_end() {
            // Stop at the start of a new item
            if matches!(self.peek().kind, TokenKind::Fn) {
                return;
            }
            self.advance();
        }
    }

    /// Parses a top-level item (function definition or script statement).
    fn parse_item(&mut self) -> Option<Item> {
        match self.peek().kind {
            TokenKind::Fn => self.parse_fn_def(),
            TokenKind::Let => self.parse_script_let(),
            _ if self.is_expr_start() => self.parse_script_expr(),
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "top-level item (function definition or statement)".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                None
            }
        }
    }

    /// Parses a top-level let statement (script mode).
    fn parse_script_let(&mut self) -> Option<Item> {
        let stmt = self.parse_let_stmt()?;
        let span = stmt.span();
        let id = self.node_id_gen.next();
        Some(Item::Script { stmt, id, span })
    }

    /// Parses a top-level expression statement (script mode).
    fn parse_script_expr(&mut self) -> Option<Item> {
        let expr = self.parse_expr();
        // Optional semicolon at top level
        self.match_token(&TokenKind::Semi);
        let span = expr.span();
        let stmt = Stmt::Expr { expr };
        let id = self.node_id_gen.next();
        Some(Item::Script { stmt, id, span })
    }

    /// Parses a function definition: `fn name(params) -> ret_type block`
    fn parse_fn_def(&mut self) -> Option<Item> {
        let start_span = self.peek().span;

        // Consume 'fn'
        self.advance();

        // Parse function name
        let name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "function name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };

        // Parse parameter list
        if self.expect(TokenKind::LParen, "'('").is_err() {
            return None;
        }

        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                // Parse parameter name
                let param_name = match &self.peek().kind {
                    TokenKind::Ident(sym) => {
                        let sym = *sym;
                        self.advance();
                        sym
                    }
                    _ => {
                        let token = self.peek().clone();
                        self.errors.push(ParseError::UnexpectedToken {
                            expected: "parameter name".to_string(),
                            found: token.kind,
                            span: token.span,
                        });
                        return None;
                    }
                };

                // Expect ':'
                if self.expect(TokenKind::Colon, "':'").is_err() {
                    return None;
                }

                // Parse parameter type
                let param_ty = self.parse_type()?;
                params.push((param_name, param_ty));

                // Check for comma or end of params
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }

        if self.expect(TokenKind::RParen, "')'").is_err() {
            return None;
        }

        // Parse optional return type
        let ret_ty = if self.match_token(&TokenKind::Arrow) {
            self.parse_type()?
        } else {
            // Default to Unit if no return type specified
            TypeAnnotation::Named(Symbol::new(0)) // Will need proper unit symbol
        };

        // Parse body (must be a block)
        let body = if self.check(&TokenKind::LBrace) {
            self.parse_block()
        } else {
            let token = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "function body (block)".to_string(),
                found: token.kind,
                span: token.span,
            });
            return None;
        };

        let span = start_span.to(body.span());
        let id = self.node_id_gen.next();

        Some(Item::FnDef {
            id,
            name,
            params,
            ret_ty,
            body,
            span,
        })
    }

    /// Parses a type annotation (M1: only named types like Int, Str, Bool, Unit).
    fn parse_type(&mut self) -> Option<TypeAnnotation> {
        match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                Some(TypeAnnotation::Named(sym))
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "type name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                None
            }
        }
    }

    /// Parses a block: `{ stmt* expr? }`
    fn parse_block(&mut self) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume '{'

        let mut stmts = Vec::new();
        let mut final_expr = None;

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            // Check if this is a let statement
            if self.check(&TokenKind::Let) {
                if let Some(stmt) = self.parse_let_stmt() {
                    stmts.push(stmt);
                }
                continue;
            }

            // Parse what looks like an expression
            if !self.is_expr_start() {
                // Skip this token and try to continue
                self.advance();
                continue;
            }

            // Parse the expression
            let expr = self.parse_expr();

            // Check if this was followed by a semicolon
            if self.match_token(&TokenKind::Semi) {
                // It's a statement
                stmts.push(Stmt::Expr { expr });
            } else if self.check(&TokenKind::RBrace) {
                // End of block - this is the final expression
                final_expr = Some(Box::new(expr));
                break;
            } else if matches!(self.peek().kind, TokenKind::Let) || self.is_expr_start() {
                // There's another statement/expression coming, so this is a statement
                stmts.push(Stmt::Expr { expr });
            } else {
                // End of expressions - this is the final one
                final_expr = Some(Box::new(expr));
                break;
            }
        }

        let end_span = if self.match_token(&TokenKind::RBrace) {
            self.tokens[self.current - 1].span
        } else {
            // Error: missing closing brace
            let token = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "'}'".to_string(),
                found: token.kind,
                span: token.span,
            });
            token.span
        };

        let span = start_span.to(end_span);
        let id = self.node_id_gen.next();

        Expr::Block {
            stmts,
            expr: final_expr,
            id,
            span,
        }
    }

    /// Returns true if the current token could start an expression.
    fn is_expr_start(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::IntLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::StrLit(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Ident(_)
                | TokenKind::LParen
                | TokenKind::LBrace
                | TokenKind::Minus
                | TokenKind::Bang
                | TokenKind::If
                | TokenKind::Return
        )
    }


    /// Parses a let statement: `let name: type = expr;`
    fn parse_let_stmt(&mut self) -> Option<Stmt> {
        let start_span = self.peek().span;
        self.advance(); // consume 'let'

        // Parse variable name
        let name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "variable name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };

        // Parse optional type annotation
        let ty = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        // Expect '='
        if self.expect(TokenKind::Eq, "'='").is_err() {
            return None;
        }

        // Parse initializer expression
        let init = self.parse_expr();

        // Optional semicolon
        self.match_token(&TokenKind::Semi);

        let span = start_span.to(init.span());
        let id = self.node_id_gen.next();

        Some(Stmt::Let {
            name,
            ty,
            init,
            id,
            span,
        })
    }


    /// Parses an expression.
    fn parse_expr(&mut self) -> Expr {
        // Handle return and if as special cases
        match self.peek().kind {
            TokenKind::Return => self.parse_return_expr(),
            TokenKind::If => self.parse_if_expr(),
            _ => self.parse_binary_expr(0),
        }
    }

    /// Parses a return expression: `return expr?`
    fn parse_return_expr(&mut self) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'return'

        // Check if there's an expression to return
        let expr = if self.is_expr_start() && !self.check(&TokenKind::Semi) {
            Some(Box::new(self.parse_expr()))
        } else {
            None
        };

        let span = if let Some(ref e) = expr {
            start_span.to(e.span())
        } else {
            start_span
        };

        let id = self.node_id_gen.next();

        Expr::Return { expr, id, span }
    }

    /// Parses an if expression: `if cond block (else (if_expr | block))?`
    fn parse_if_expr(&mut self) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'if'

        // Parse condition
        let cond = Box::new(self.parse_binary_expr(0));

        // Parse then branch (must be a block)
        let then_branch = if self.check(&TokenKind::LBrace) {
            Box::new(self.parse_block())
        } else {
            let token = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "block for 'if' then branch".to_string(),
                found: token.kind,
                span: token.span,
            });
            // Create a dummy block for error recovery
            let id = self.node_id_gen.next();
            Box::new(Expr::Block {
                stmts: vec![],
                expr: None,
                id,
                span: token.span,
            })
        };

        // Parse optional else branch
        let else_branch = if self.match_token(&TokenKind::Else) {
            Some(if self.check(&TokenKind::If) {
                Box::new(self.parse_if_expr())
            } else if self.check(&TokenKind::LBrace) {
                Box::new(self.parse_block())
            } else {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "block or 'if' after 'else'".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                // Create a dummy block for error recovery
                let id = self.node_id_gen.next();
                Box::new(Expr::Block {
                    stmts: vec![],
                    expr: None,
                    id,
                    span: token.span,
                })
            })
        } else {
            None
        };

        let span = if let Some(ref e) = else_branch {
            start_span.to(e.span())
        } else {
            start_span.to(then_branch.span())
        };

        let id = self.node_id_gen.next();

        Expr::If {
            cond,
            then_branch,
            else_branch,
            id,
            span,
        }
    }

    /// Parses binary expressions using precedence climbing.
    fn parse_binary_expr(&mut self, min_prec: u8) -> Expr {
        let mut left = self.parse_unary_expr();

        loop {
            // Check if current token is a binary operator
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::BangEq => BinOp::Ne,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::LtEq => BinOp::Le,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::GtEq => BinOp::Ge,
                TokenKind::AndAnd => BinOp::And,
                TokenKind::OrOr => BinOp::Or,
                _ => break,
            };

            let prec = op.precedence();
            if prec < min_prec {
                break;
            }

            self.advance(); // consume operator

            let right = self.parse_binary_expr(prec + 1);

            let span = left.span().to(right.span());
            let id = self.node_id_gen.next();

            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                id,
                span,
            };
        }

        left
    }

    /// Parses unary expressions: `-expr` or `!expr`
    fn parse_unary_expr(&mut self) -> Expr {
        match self.peek().kind {
            TokenKind::Minus => {
                let start_span = self.peek().span;
                self.advance();
                let expr = Box::new(self.parse_unary_expr());
                let span = start_span.to(expr.span());
                let id = self.node_id_gen.next();
                Expr::Unary {
                    op: UnOp::Neg,
                    expr,
                    id,
                    span,
                }
            }
            TokenKind::Bang => {
                let start_span = self.peek().span;
                self.advance();
                let expr = Box::new(self.parse_unary_expr());
                let span = start_span.to(expr.span());
                let id = self.node_id_gen.next();
                Expr::Unary {
                    op: UnOp::Not,
                    expr,
                    id,
                    span,
                }
            }
            _ => self.parse_call_expr(),
        }
    }

    /// Parses call expressions: `primary ( args )*`
    fn parse_call_expr(&mut self) -> Expr {
        let mut expr = self.parse_primary();

        loop {
            if self.match_token(&TokenKind::LParen) {
                let mut args = Vec::new();

                if !self.check(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr());

                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }

                let end_span = if let Ok(token) = self.expect(TokenKind::RParen, "')'") {
                    token.span
                } else {
                    self.peek().span
                };

                let span = expr.span().to(end_span);
                let id = self.node_id_gen.next();

                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    id,
                    span,
                };
            } else {
                break;
            }
        }

        expr
    }

    /// Parses primary expressions: literals, variables, parenthesized expressions, blocks.
    fn parse_primary(&mut self) -> Expr {
        let token = self.peek().clone();
        let id = self.node_id_gen.next();

        match token.kind {
            TokenKind::IntLit(n) => {
                self.advance();
                Expr::Literal {
                    value: Literal::Int(n),
                    id,
                    span: token.span,
                }
            }
            TokenKind::FloatLit(_f) => {
                self.advance();
                // M1 doesn't support floats in the grammar, but we handle them anyway
                // For now, treat them as errors
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "expression".to_string(),
                    found: token.kind.clone(),
                    span: token.span,
                });
                // Return a dummy expression
                Expr::Literal {
                    value: Literal::Int(0),
                    id,
                    span: token.span,
                }
            }
            TokenKind::StrLit(sym) => {
                self.advance();
                Expr::Literal {
                    value: Literal::Str(sym),
                    id,
                    span: token.span,
                }
            }
            TokenKind::True => {
                self.advance();
                Expr::Literal {
                    value: Literal::Bool(true),
                    id,
                    span: token.span,
                }
            }
            TokenKind::False => {
                self.advance();
                Expr::Literal {
                    value: Literal::Bool(false),
                    id,
                    span: token.span,
                }
            }
            TokenKind::Ident(name) => {
                self.advance();
                Expr::Variable {
                    name,
                    id,
                    span: token.span,
                }
            }
            TokenKind::LParen => {
                self.advance();

                // Check for unit literal `()`
                if self.match_token(&TokenKind::RParen) {
                    let span = token.span.to(self.tokens[self.current - 1].span);
                    return Expr::Literal {
                        value: Literal::Unit,
                        id,
                        span,
                    };
                }

                // Otherwise, parse parenthesized expression
                let expr = self.parse_expr();

                if let Err(()) = self.expect(TokenKind::RParen, "')'") {
                    // Error already recorded, return the expression anyway
                }

                expr
            }
            TokenKind::LBrace => self.parse_block(),
            _ => {
                self.errors.push(ParseError::ExpectedExpression {
                    found: token.kind,
                    span: token.span,
                });
                // Return a dummy expression for error recovery
                Expr::Literal {
                    value: Literal::Unit,
                    id,
                    span: token.span,
                }
            }
        }
    }
}

/// Parses a LexResult into a ParseResult.
///
/// This is the only public entry point to the parser.
///
/// # Arguments
///
/// * `lex` - The result from the lexer stage
///
/// # Returns
///
/// A ParseResult containing the AST items and any parse errors encountered.
pub fn parse(lex: &LexResult) -> ParseResult {
    let mut parser = Parser::new(&lex.tokens);
    let items = parser.parse_program();
    ParseResult::new(items, parser.errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Interner, Span};

    /// Helper to create a simple lex result for testing
    fn make_lex_result(tokens: Vec<Token>) -> LexResult {
        LexResult::new(tokens, vec![])
    }

    /// Helper to create a token
    fn tok(kind: TokenKind, start: u32, end: u32) -> Token {
        Token::new(kind, Span::new(start, end))
    }

    #[test]
    fn test_empty_program() {
        let lex = make_lex_result(vec![tok(TokenKind::Eof, 0, 0)]);
        let result = parse(&lex);
        assert_eq!(result.items.len(), 0);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_simple_function() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");

        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::RBrace, 10, 11),
            tok(TokenKind::Eof, 11, 11),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 0);

        match &result.items[0] {
            Item::FnDef { name, params, body, .. } => {
                assert_eq!(*name, foo);
                assert_eq!(params.len(), 0);
                match body {
                    Expr::Block { stmts, expr, .. } => {
                        assert_eq!(stmts.len(), 0);
                        assert!(expr.is_none());
                    }
                    _ => panic!("Expected block"),
                }
            }
            Item::Script { .. } => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_function_with_params() {
        let mut interner = Interner::new();
        let add = interner.intern("add");
        let x = interner.intern("x");
        let y = interner.intern("y");
        let int_ty = interner.intern("Int");

        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(add), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::Ident(x), 7, 8),
            tok(TokenKind::Colon, 8, 9),
            tok(TokenKind::Ident(int_ty), 10, 13),
            tok(TokenKind::Comma, 13, 14),
            tok(TokenKind::Ident(y), 15, 16),
            tok(TokenKind::Colon, 16, 17),
            tok(TokenKind::Ident(int_ty), 18, 21),
            tok(TokenKind::RParen, 21, 22),
            tok(TokenKind::Arrow, 23, 25),
            tok(TokenKind::Ident(int_ty), 26, 29),
            tok(TokenKind::LBrace, 30, 31),
            tok(TokenKind::Ident(x), 32, 33),
            tok(TokenKind::Plus, 34, 35),
            tok(TokenKind::Ident(y), 36, 37),
            tok(TokenKind::RBrace, 38, 39),
            tok(TokenKind::Eof, 39, 39),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 0);

        match &result.items[0] {
            Item::FnDef { name, params, ret_ty, .. } => {
                assert_eq!(*name, add);
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].0, x);
                assert_eq!(params[1].0, y);
                assert_eq!(*ret_ty, TypeAnnotation::Named(int_ty));
            }
            Item::Script { .. } => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_binary_precedence() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");

        // fn foo() { 1 + 2 * 3 }
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::IntLit(1), 11, 12),
            tok(TokenKind::Plus, 13, 14),
            tok(TokenKind::IntLit(2), 15, 16),
            tok(TokenKind::Star, 17, 18),
            tok(TokenKind::IntLit(3), 19, 20),
            tok(TokenKind::RBrace, 21, 22),
            tok(TokenKind::Eof, 22, 22),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 0);

        match &result.items[0] {
            Item::FnDef { body, .. } => {
                match body {
                    Expr::Block { expr: Some(expr), .. } => {
                        // Should be: 1 + (2 * 3)
                        match expr.as_ref() {
                            Expr::Binary { op: BinOp::Add, left, right, .. } => {
                                // Left should be literal 1
                                match left.as_ref() {
                                    Expr::Literal { value: Literal::Int(1), .. } => {}
                                    _ => panic!("Expected literal 1"),
                                }
                                // Right should be 2 * 3
                                match right.as_ref() {
                                    Expr::Binary { op: BinOp::Mul, .. } => {}
                                    _ => panic!("Expected multiplication"),
                                }
                            }
                            _ => panic!("Expected addition at top level"),
                        }
                    }
                    _ => panic!("Expected block with expression"),
                }
            }
            Item::Script { .. } => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_let_binding() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        let x = interner.intern("x");
        let int_ty = interner.intern("Int");

        // fn foo() { let x: Int = 5; }
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::Let, 11, 14),
            tok(TokenKind::Ident(x), 15, 16),
            tok(TokenKind::Colon, 16, 17),
            tok(TokenKind::Ident(int_ty), 18, 21),
            tok(TokenKind::Eq, 22, 23),
            tok(TokenKind::IntLit(5), 24, 25),
            tok(TokenKind::Semi, 25, 26),
            tok(TokenKind::RBrace, 27, 28),
            tok(TokenKind::Eof, 28, 28),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_if_expression() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        let x = interner.intern("x");

        // fn foo() { if x { 1 } else { 2 } }
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::If, 11, 13),
            tok(TokenKind::Ident(x), 14, 15),
            tok(TokenKind::LBrace, 16, 17),
            tok(TokenKind::IntLit(1), 18, 19),
            tok(TokenKind::RBrace, 20, 21),
            tok(TokenKind::Else, 22, 26),
            tok(TokenKind::LBrace, 27, 28),
            tok(TokenKind::IntLit(2), 29, 30),
            tok(TokenKind::RBrace, 31, 32),
            tok(TokenKind::RBrace, 33, 34),
            tok(TokenKind::Eof, 34, 34),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_function_call() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        let bar = interner.intern("bar");

        // fn foo() { bar(1, 2) }
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::Ident(bar), 11, 14),
            tok(TokenKind::LParen, 14, 15),
            tok(TokenKind::IntLit(1), 15, 16),
            tok(TokenKind::Comma, 16, 17),
            tok(TokenKind::IntLit(2), 18, 19),
            tok(TokenKind::RParen, 19, 20),
            tok(TokenKind::RBrace, 21, 22),
            tok(TokenKind::Eof, 22, 22),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 0);

        match &result.items[0] {
            Item::FnDef { body, .. } => match body {
                Expr::Block { expr: Some(expr), .. } => match expr.as_ref() {
                    Expr::Call { args, .. } => {
                        assert_eq!(args.len(), 2);
                    }
                    _ => panic!("Expected call expression"),
                },
                _ => panic!("Expected block with expression"),
            },
            Item::Script { .. } => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_return_statement() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        let x = interner.intern("x");

        // fn foo() { return x + 1; }
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::Return, 11, 17),
            tok(TokenKind::Ident(x), 18, 19),
            tok(TokenKind::Plus, 20, 21),
            tok(TokenKind::IntLit(1), 22, 23),
            tok(TokenKind::Semi, 23, 24),
            tok(TokenKind::RBrace, 25, 26),
            tok(TokenKind::Eof, 26, 26),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_error_recovery_missing_semicolon() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        let x = interner.intern("x");

        // fn foo() { let x = 5 let y = 6; } - missing semicolon after first let
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::Let, 11, 14),
            tok(TokenKind::Ident(x), 15, 16),
            tok(TokenKind::Eq, 17, 18),
            tok(TokenKind::IntLit(5), 19, 20),
            // Missing semicolon here
            tok(TokenKind::Let, 21, 24),
            tok(TokenKind::Ident(x), 25, 26),
            tok(TokenKind::Eq, 27, 28),
            tok(TokenKind::IntLit(6), 29, 30),
            tok(TokenKind::Semi, 30, 31),
            tok(TokenKind::RBrace, 32, 33),
            tok(TokenKind::Eof, 33, 33),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        // Parser should continue and parse both statements (error recovery)
        assert_eq!(result.items.len(), 1);
        // Should have recovered without panicking
    }

    #[test]
    fn test_error_recovery_missing_brace() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");

        // fn foo() { 42 - missing closing brace
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::IntLit(42), 11, 13),
            tok(TokenKind::Eof, 13, 13),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        // Parser should not panic
        assert_eq!(result.items.len(), 1);
        // Should have an error about missing brace
        assert!(result.errors.len() > 0);
    }

    #[test]
    fn test_error_recovery_unexpected_token() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");

        // fn foo() { % } - % is not a valid expression start
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::Percent, 11, 12),
            tok(TokenKind::RBrace, 13, 14),
            tok(TokenKind::Eof, 14, 14),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        // Parser should not panic
        assert_eq!(result.items.len(), 1);
    }
}
