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
    BinOp, Expr, ImplMethod, Item, LexResult, Literal, MatchArm, NamedArg, NodeId, Param,
    ParseError, ParseResult, Pattern, RequireMode, RequireStmt, ShellPart, ShellTokenPart,
    Stmt, Symbol, Token, TokenKind, TraitMethod, TypeAnnotation, TypeParam, UnOp,
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
    /// When true, an `Ident { ... }` expression is NOT parsed as a struct
    /// literal — it's left for an outer construct (e.g. `if cond { ... }`,
    /// `while cond { ... }`, `match scrutinee { ... }`) to consume the `{`.
    /// Mirrors rustc's `Restrictions::NO_STRUCT_LITERAL`.
    no_struct_literal: bool,
}

impl<'a> Parser<'a> {
    /// Creates a new parser for the given tokens.
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            current: 0,
            node_id_gen: NodeIdGen::new(),
            errors: Vec::new(),
            no_struct_literal: false,
        }
    }

    /// Runs `f` with `no_struct_literal` set, restoring the previous state on
    /// return. Used when parsing the head of `if`/`while`/`match` so that
    /// `Ident { ... }` is not greedily consumed as a struct literal.
    fn with_no_struct_literal<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.no_struct_literal;
        self.no_struct_literal = true;
        let result = f(self);
        self.no_struct_literal = prev;
        result
    }

    /// Runs `f` with struct literals re-allowed (e.g. inside parens, `match`
    /// arm bodies, function arguments).
    fn with_struct_literal_allowed<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.no_struct_literal;
        self.no_struct_literal = false;
        let result = f(self);
        self.no_struct_literal = prev;
        result
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
            if matches!(
                self.peek().kind,
                TokenKind::Fn
                    | TokenKind::Struct
                    | TokenKind::Enum
                    | TokenKind::Trait
                    | TokenKind::Impl
            ) {
                return;
            }
            self.advance();
        }
    }

    /// Parses a top-level item (function definition or script statement).
    fn parse_item(&mut self) -> Option<Item> {
        match self.peek().kind {
            TokenKind::Fn => self.parse_fn_def(),
            TokenKind::Struct => self.parse_struct_def(),
            TokenKind::Enum => self.parse_enum_def(),
            TokenKind::Trait => self.parse_trait_def(),
            TokenKind::Impl => self.parse_impl_block(),
            TokenKind::Let => self.parse_script_let(),
            TokenKind::Require => self.parse_script_require(),
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

    /// Parses a struct definition: `struct Name { field: Type, ... }`
    fn parse_struct_def(&mut self) -> Option<Item> {
        let start_span = self.peek().span;
        self.advance(); // consume 'struct'

        let name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "struct name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };

        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            return None;
        }

        let mut fields: Vec<(Symbol, TypeAnnotation)> = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field_name = match &self.peek().kind {
                TokenKind::Ident(sym) => {
                    let sym = *sym;
                    self.advance();
                    sym
                }
                _ => {
                    let tok = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "field name".to_string(),
                        found: tok.kind,
                        span: tok.span,
                    });
                    break;
                }
            };

            if self.expect(TokenKind::Colon, "':'").is_err() {
                break;
            }
            let ty = match self.parse_type() {
                Some(t) => t,
                None => break,
            };
            fields.push((field_name, ty));

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
            tok.span
        } else {
            self.peek().span
        };

        let span = start_span.to(end_span);
        let id = self.node_id_gen.next();
        Some(Item::StructDef { id, name, fields, span })
    }

    /// Parses an enum definition: `enum Name { Variant(Types), ... }`
    fn parse_enum_def(&mut self) -> Option<Item> {
        let start_span = self.peek().span;
        self.advance(); // consume 'enum'

        let name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "enum name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };

        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            return None;
        }

        let mut variants: Vec<(Symbol, Vec<TypeAnnotation>)> = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let variant_name = match &self.peek().kind {
                TokenKind::Ident(sym) => {
                    let sym = *sym;
                    self.advance();
                    sym
                }
                _ => {
                    let tok = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "variant name".to_string(),
                        found: tok.kind,
                        span: tok.span,
                    });
                    break;
                }
            };

            let mut payload: Vec<TypeAnnotation> = Vec::new();
            if self.match_token(&TokenKind::LParen) {
                if !self.check(&TokenKind::RParen) {
                    loop {
                        match self.parse_type() {
                            Some(t) => payload.push(t),
                            None => break,
                        }
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                if self.expect(TokenKind::RParen, "')'").is_err() {
                    break;
                }
            }

            variants.push((variant_name, payload));

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
            tok.span
        } else {
            self.peek().span
        };

        let span = start_span.to(end_span);
        let id = self.node_id_gen.next();
        Some(Item::EnumDef { id, name, variants, span })
    }

    /// Parses a top-level require statement (script mode).
    fn parse_script_require(&mut self) -> Option<Item> {
        let stmt = self.parse_require_stmt()?;
        let span = stmt.span();
        let id = self.node_id_gen.next();
        Some(Item::Script { stmt, id, span })
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

    /// Parses a function definition: `fn name<T: Bound>(params) -> ret_type block`
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

        // Optional generic parameter list: `<T, U: Bound + Other>`
        let type_params = if self.check(&TokenKind::Lt) {
            self.parse_type_params()
        } else {
            Vec::new()
        };

        // Parse parameter list
        if self.expect(TokenKind::LParen, "'('").is_err() {
            return None;
        }

        let mut params: Vec<Param> = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let param_start = self.peek().span;

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

                // Span of type token (last consumed token after parse_type)
                let ty_end_span = self.tokens[self.current - 1].span;

                // Parse optional default: `= expr`
                let default = if self.match_token(&TokenKind::Eq) {
                    Some(Box::new(self.parse_expr()))
                } else {
                    None
                };

                let param_span = param_start.to(
                    default.as_ref().map(|d| d.span()).unwrap_or(ty_end_span)
                );
                params.push(Param {
                    span: param_span,
                    name: param_name,
                    ty: param_ty,
                    default,
                });

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
            type_params,
            params,
            ret_ty,
            body,
            span,
        })
    }

    /// Parses a generic parameter list: `<T, U: Trait, V: A + B>`.
    fn parse_type_params(&mut self) -> Vec<TypeParam> {
        let _ = self.expect(TokenKind::Lt, "'<'");
        let mut params: Vec<TypeParam> = Vec::new();

        while !self.check(&TokenKind::Gt) && !self.is_at_end() {
            let start_span = self.peek().span;
            let name = match &self.peek().kind {
                TokenKind::Ident(sym) => {
                    let sym = *sym;
                    self.advance();
                    sym
                }
                _ => {
                    let token = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "type parameter name".to_string(),
                        found: token.kind,
                        span: token.span,
                    });
                    break;
                }
            };

            let mut bounds: Vec<Symbol> = Vec::new();
            let mut end_span = self.tokens[self.current - 1].span;
            if self.match_token(&TokenKind::Colon) {
                loop {
                    match &self.peek().kind {
                        TokenKind::Ident(sym) => {
                            let sym = *sym;
                            end_span = self.peek().span;
                            self.advance();
                            bounds.push(sym);
                        }
                        _ => {
                            let token = self.peek().clone();
                            self.errors.push(ParseError::UnexpectedToken {
                                expected: "trait bound name".to_string(),
                                found: token.kind,
                                span: token.span,
                            });
                            break;
                        }
                    }
                    if !self.match_token(&TokenKind::Plus) {
                        break;
                    }
                }
            }

            params.push(TypeParam {
                name,
                bounds,
                span: start_span.to(end_span),
            });

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        let _ = self.expect(TokenKind::Gt, "'>'");
        params
    }

    /// Parses a trait definition: `trait Name { fn method(self, params) -> Ret; ... }`
    fn parse_trait_def(&mut self) -> Option<Item> {
        let start_span = self.peek().span;
        self.advance(); // consume 'trait'

        let name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "trait name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };

        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            return None;
        }

        let mut methods: Vec<TraitMethod> = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let m_start = self.peek().span;
            if self.expect(TokenKind::Fn, "'fn'").is_err() {
                break;
            }
            let m_name = match &self.peek().kind {
                TokenKind::Ident(sym) => {
                    let sym = *sym;
                    self.advance();
                    sym
                }
                _ => {
                    let token = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "method name".to_string(),
                        found: token.kind,
                        span: token.span,
                    });
                    break;
                }
            };

            let params = match self.parse_method_params() {
                Some(p) => p,
                None => break,
            };

            let ret_ty = if self.match_token(&TokenKind::Arrow) {
                match self.parse_type() {
                    Some(t) => t,
                    None => break,
                }
            } else {
                TypeAnnotation::Named(Symbol::new(0))
            };

            // Optional ; or , between methods
            let _ = self.match_token(&TokenKind::Semi)
                || self.match_token(&TokenKind::Comma);

            let m_span = m_start.to(self.tokens[self.current - 1].span);
            let id = self.node_id_gen.next();
            methods.push(TraitMethod {
                id,
                name: m_name,
                params,
                ret_ty,
                span: m_span,
            });
        }

        let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
            tok.span
        } else {
            self.peek().span
        };

        let span = start_span.to(end_span);
        let id = self.node_id_gen.next();
        Some(Item::TraitDef { id, name, methods, span })
    }

    /// Parses an impl block: `impl Trait for Type { fn method(self, ...) { body } ... }`.
    fn parse_impl_block(&mut self) -> Option<Item> {
        let start_span = self.peek().span;
        self.advance(); // consume 'impl'

        let trait_name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "trait name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };

        if self.expect(TokenKind::For, "'for'").is_err() {
            return None;
        }

        let type_name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "type name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };

        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            return None;
        }

        let mut methods: Vec<ImplMethod> = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            match self.parse_impl_method() {
                Some(m) => methods.push(m),
                None => break,
            }
        }

        let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
            tok.span
        } else {
            self.peek().span
        };

        let span = start_span.to(end_span);
        let id = self.node_id_gen.next();
        Some(Item::ImplBlock {
            id,
            trait_name,
            type_name,
            methods,
            span,
        })
    }

    /// Parses one impl method: `fn name(params) -> Ret { body }`. Identical
    /// to a free-standing `fn` except it lives inside an impl block and is
    /// represented by an `ImplMethod`, not `Item::FnDef`.
    fn parse_impl_method(&mut self) -> Option<ImplMethod> {
        let start_span = self.peek().span;
        if self.expect(TokenKind::Fn, "'fn'").is_err() {
            return None;
        }
        let name = match &self.peek().kind {
            TokenKind::Ident(sym) => {
                let sym = *sym;
                self.advance();
                sym
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "method name".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                return None;
            }
        };
        let params = self.parse_method_params()?;
        let ret_ty = if self.match_token(&TokenKind::Arrow) {
            self.parse_type()?
        } else {
            TypeAnnotation::Named(Symbol::new(0))
        };
        let body = if self.check(&TokenKind::LBrace) {
            self.parse_block()
        } else {
            let token = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "method body (block)".to_string(),
                found: token.kind,
                span: token.span,
            });
            return None;
        };
        let span = start_span.to(body.span());
        let id = self.node_id_gen.next();
        Some(ImplMethod {
            id,
            name,
            params,
            ret_ty,
            body,
            span,
        })
    }

    /// Parses `(self [: Ty], param: Ty, ...)` for a method. The `self`
    /// parameter is treated as if it had the impl's enclosing type, but
    /// for parsing simplicity its declared type is the literal name `Self`.
    /// Trait-method signatures and impl-method bodies share this format.
    fn parse_method_params(&mut self) -> Option<Vec<Param>> {
        if self.expect(TokenKind::LParen, "'('").is_err() {
            return None;
        }

        let mut params: Vec<Param> = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let p_start = self.peek().span;
                let name = match &self.peek().kind {
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

                // For non-self params, expect `: Ty`. For `self`, the type is
                // implicit (equal to `Self`); allow either an explicit
                // annotation or just the bare name.
                let ty = if self.match_token(&TokenKind::Colon) {
                    self.parse_type()?
                } else {
                    // Bare `self` — annotate with the literal `Self` symbol
                    // so the resolver/type-checker can substitute it later.
                    TypeAnnotation::Named(name)
                };
                let p_end_span = self.tokens[self.current - 1].span;
                let p_span = p_start.to(p_end_span);
                params.push(Param {
                    span: p_span,
                    name,
                    ty,
                    default: None,
                });

                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }

        if self.expect(TokenKind::RParen, "')'").is_err() {
            return None;
        }
        Some(params)
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

            // Check if this is a require statement
            if self.check(&TokenKind::Require) {
                if let Some(stmt) = self.parse_require_stmt() {
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

            // Check if this might be an assignment (identifier followed by =)
            if matches!(self.peek().kind, TokenKind::Ident(_)) {
                // Lookahead to check for '='
                if self.current + 1 < self.tokens.len()
                    && matches!(self.tokens[self.current + 1].kind, TokenKind::Eq)
                {
                    // Parse as assignment statement
                    if let Some(stmt) = self.parse_assign_stmt() {
                        stmts.push(stmt);
                    }
                    continue;
                }
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
                | TokenKind::While
                | TokenKind::Loop
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Return
                | TokenKind::Match
                | TokenKind::OrOr  // closure: || { body }
                | TokenKind::ShellLine(_)
        )
    }


    /// Parses an assignment statement: `name = expr;`
    fn parse_assign_stmt(&mut self) -> Option<Stmt> {
        let start_span = self.peek().span;

        // Parse target (currently only variables)
        let target = self.parse_primary();

        // Expect '='
        if self.expect(TokenKind::Eq, "'='").is_err() {
            return None;
        }

        // Parse value expression
        let value = self.parse_expr();

        // Optional semicolon
        self.match_token(&TokenKind::Semi);

        let span = start_span.to(value.span());
        let id = self.node_id_gen.next();

        Some(Stmt::Assign {
            target,
            value,
            id,
            span,
        })
    }

    /// Parses a let statement: `let name: type = expr;` or `let mut name: type = expr;`
    fn parse_let_stmt(&mut self) -> Option<Stmt> {
        let start_span = self.peek().span;
        self.advance(); // consume 'let'

        // Check for 'mut' keyword
        let mutable = if self.match_token(&TokenKind::Mut) {
            true
        } else {
            false
        };

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
            mutable,
            ty,
            init,
            id,
            span,
        })
    }


    /// Parses an expression.
    fn parse_expr(&mut self) -> Expr {
        // Handle control flow keywords as special cases
        match self.peek().kind {
            TokenKind::Return => self.parse_return_expr(),
            TokenKind::If => self.parse_if_expr(),
            TokenKind::While => self.parse_while_expr(),
            TokenKind::Loop => self.parse_loop_expr(),
            TokenKind::Break => self.parse_break_expr(),
            TokenKind::Continue => self.parse_continue_expr(),
            TokenKind::Match => self.parse_match_expr(),
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

        // Parse condition (no struct literals — `{` belongs to the then-branch)
        let cond = Box::new(self.with_no_struct_literal(|p| p.parse_binary_expr(0)));

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

    /// Parses a while loop: `while cond block`
    fn parse_while_expr(&mut self) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'while'

        // Parse condition (no struct literals — `{` belongs to the body)
        let cond = Box::new(self.with_no_struct_literal(|p| p.parse_binary_expr(0)));

        // Parse body (must be a block)
        let body = if self.check(&TokenKind::LBrace) {
            Box::new(self.parse_block())
        } else {
            let token = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "block for 'while' body".to_string(),
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

        let span = start_span.to(body.span());
        let id = self.node_id_gen.next();

        Expr::While { cond, body, id, span }
    }

    /// Parses an infinite loop: `loop block`
    fn parse_loop_expr(&mut self) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'loop'

        // Parse body (must be a block)
        let body = if self.check(&TokenKind::LBrace) {
            Box::new(self.parse_block())
        } else {
            let token = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "block for 'loop' body".to_string(),
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

        let span = start_span.to(body.span());
        let id = self.node_id_gen.next();

        Expr::Loop { body, id, span }
    }

    /// Parses a break expression: `break`
    fn parse_break_expr(&mut self) -> Expr {
        let span = self.peek().span;
        self.advance(); // consume 'break'
        let id = self.node_id_gen.next();

        Expr::Break { id, span }
    }

    /// Parses a continue expression: `continue`
    fn parse_continue_expr(&mut self) -> Expr {
        let span = self.peek().span;
        self.advance(); // consume 'continue'
        let id = self.node_id_gen.next();

        Expr::Continue { id, span }
    }

    /// Parses a match expression: `match scrutinee { pattern => body, ... }`
    fn parse_match_expr(&mut self) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'match'

        // Scrutinee: forbid struct literals so `match x {` doesn't parse `x { ... }`
        // as a struct literal.
        let scrutinee = Box::new(self.with_no_struct_literal(|p| p.parse_binary_expr(0)));

        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            let id = self.node_id_gen.next();
            return Expr::Match {
                scrutinee,
                arms: Vec::new(),
                id,
                span: start_span,
            };
        }

        let mut arms: Vec<MatchArm> = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let arm_start = self.peek().span;
            let pattern = self.parse_pattern();
            if self.expect(TokenKind::FatArrow, "'=>'").is_err() {
                break;
            }
            let body = self.with_struct_literal_allowed(|p| p.parse_expr());
            let span = arm_start.to(body.span());
            arms.push(MatchArm { pattern, body, span });

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
            tok.span
        } else {
            self.peek().span
        };

        let span = start_span.to(end_span);
        let id = self.node_id_gen.next();
        Expr::Match { scrutinee, arms, id, span }
    }

    /// Parses a single pattern.
    fn parse_pattern(&mut self) -> Pattern {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Underscore => {
                self.advance();
                Pattern::Wildcard { span: token.span }
            }
            TokenKind::IntLit(n) => {
                self.advance();
                Pattern::Literal {
                    value: Literal::Int(n),
                    span: token.span,
                }
            }
            TokenKind::FloatLit(f) => {
                self.advance();
                Pattern::Literal {
                    value: Literal::Float(f),
                    span: token.span,
                }
            }
            TokenKind::StrLit(sym) => {
                self.advance();
                Pattern::Literal {
                    value: Literal::Str(sym),
                    span: token.span,
                }
            }
            TokenKind::True => {
                self.advance();
                Pattern::Literal {
                    value: Literal::Bool(true),
                    span: token.span,
                }
            }
            TokenKind::False => {
                self.advance();
                Pattern::Literal {
                    value: Literal::Bool(false),
                    span: token.span,
                }
            }
            TokenKind::LParen => {
                let start = token.span;
                self.advance(); // '('
                let mut patterns: Vec<Pattern> = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        patterns.push(self.parse_pattern());
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end_span = if let Ok(tok) = self.expect(TokenKind::RParen, "')'") {
                    tok.span
                } else {
                    self.peek().span
                };
                Pattern::Tuple {
                    patterns,
                    span: start.to(end_span),
                }
            }
            TokenKind::Ident(name) => {
                self.advance();
                // Variant pattern: `EnumName::Variant(patterns)` or `EnumName::Variant`
                if self.match_token(&TokenKind::ColonColon) {
                    let variant = match self.peek().kind {
                        TokenKind::Ident(s) => {
                            self.advance();
                            s
                        }
                        _ => {
                            let tok = self.peek().clone();
                            self.errors.push(ParseError::UnexpectedToken {
                                expected: "variant name".to_string(),
                                found: tok.kind,
                                span: tok.span,
                            });
                            return Pattern::Wildcard { span: token.span };
                        }
                    };
                    let mut patterns: Vec<Pattern> = Vec::new();
                    let mut end_span = self.tokens[self.current - 1].span;
                    if self.match_token(&TokenKind::LParen) {
                        if !self.check(&TokenKind::RParen) {
                            loop {
                                patterns.push(self.parse_pattern());
                                if !self.match_token(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        if let Ok(tok) = self.expect(TokenKind::RParen, "')'") {
                            end_span = tok.span;
                        }
                    }
                    return Pattern::Variant {
                        enum_name: name,
                        variant,
                        patterns,
                        span: token.span.to(end_span),
                    };
                }
                // Struct pattern: `StructName { field: pat, ... }`
                if self.check(&TokenKind::LBrace) {
                    self.advance(); // '{'
                    let mut fields: Vec<(Symbol, Pattern)> = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                        let field_name = match self.peek().kind {
                            TokenKind::Ident(s) => {
                                self.advance();
                                s
                            }
                            _ => {
                                let tok = self.peek().clone();
                                self.errors.push(ParseError::UnexpectedToken {
                                    expected: "field name".to_string(),
                                    found: tok.kind,
                                    span: tok.span,
                                });
                                break;
                            }
                        };
                        // `field: pat` or shorthand `field` (treated as variable
                        // pattern with the same name).
                        let pat = if self.match_token(&TokenKind::Colon) {
                            self.parse_pattern()
                        } else {
                            let id = self.node_id_gen.next();
                            Pattern::Variable {
                                name: field_name,
                                id,
                                span: self.tokens[self.current - 1].span,
                            }
                        };
                        fields.push((field_name, pat));
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
                        tok.span
                    } else {
                        self.peek().span
                    };
                    return Pattern::Struct {
                        name,
                        fields,
                        span: token.span.to(end_span),
                    };
                }
                // Otherwise, a plain variable binding pattern.
                let id = self.node_id_gen.next();
                Pattern::Variable {
                    name,
                    id,
                    span: token.span,
                }
            }
            _ => {
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "pattern".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                self.advance();
                Pattern::Wildcard { span: token.span }
            }
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

    /// Returns true if the current position looks like the start of a named arg (ident ':').
    fn is_named_arg_start(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_))
            && self.current + 1 < self.tokens.len()
            && matches!(self.tokens[self.current + 1].kind, TokenKind::Colon)
    }

    /// Checks if the upcoming `{ ... }` is a struct literal body.
    ///
    /// Called after we've consumed `Ident` and seen `{`. Disambiguates against
    /// expression blocks. A struct literal body is:
    ///   - empty:       `Name {}`
    ///   - has fields:  `Name { ident: ...`
    ///
    /// Anything else (e.g. `{ stmt; ... }` or `{ expr }`) is treated as a block.
    fn looks_like_struct_lit_body(&self) -> bool {
        // We expect `self.peek().kind` to be `LBrace`.
        let next = self.tokens.get(self.current + 1).map(|t| &t.kind);
        match next {
            Some(TokenKind::RBrace) => true, // empty `Name {}`
            Some(TokenKind::Ident(_)) => {
                // `Name { ident :` — looks like a struct field
                matches!(
                    self.tokens.get(self.current + 2).map(|t| &t.kind),
                    Some(TokenKind::Colon)
                )
            }
            _ => false,
        }
    }

    /// Parses call expressions: `primary ( args )*` and field access `expr.ident`.
    ///
    /// Function arguments must be named (`name: value`). Positional args emit
    /// `ParseError::PositionalArg` and continue parsing for recovery.
    fn parse_call_expr(&mut self) -> Expr {
        let mut expr = self.parse_primary();

        loop {
            if self.match_token(&TokenKind::LParen) {
                let mut args: Vec<NamedArg> = Vec::new();

                if !self.check(&TokenKind::RParen) {
                    loop {
                        let arg_start = self.peek().span;

                        if self.is_named_arg_start() {
                            // Named arg: consume `name ':'`
                            let name = if let TokenKind::Ident(sym) = self.peek().kind {
                                let sym = sym;
                                self.advance(); // consume name
                                self.advance(); // consume ':'
                                sym
                            } else {
                                unreachable!()
                            };
                            let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                            let span = arg_start.to(value.span());
                            args.push(NamedArg { span, name, value: Box::new(value) });
                        } else {
                            // Positional arg — error and recover
                            self.errors.push(ParseError::PositionalArg { span: arg_start });
                            let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                            let span = arg_start.to(value.span());
                            // Use a sentinel name (sym 0) for error recovery
                            args.push(NamedArg { span, name: Symbol::new(0), value: Box::new(value) });
                        }

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
            } else if self.check(&TokenKind::Dot) {
                self.advance(); // '.'
                let field_tok = self.peek().clone();
                let field = match field_tok.kind {
                    TokenKind::Ident(s) => {
                        self.advance();
                        s
                    }
                    _ => {
                        self.errors.push(ParseError::UnexpectedToken {
                            expected: "field name after '.'".to_string(),
                            found: field_tok.kind,
                            span: field_tok.span,
                        });
                        break;
                    }
                };
                if self.match_token(&TokenKind::LParen) {
                    // Method call: receiver.method(args)
                    let args = self.parse_method_args();
                    let end_span = if let Ok(tok) = self.expect(TokenKind::RParen, "')'") {
                        tok.span
                    } else {
                        self.peek().span
                    };
                    let span = expr.span().to(end_span);
                    let id = self.node_id_gen.next();
                    expr = Expr::MethodCall {
                        receiver: Box::new(expr),
                        method: field,
                        args,
                        id,
                        span,
                    };
                } else {
                    let span = expr.span().to(field_tok.span);
                    let id = self.node_id_gen.next();
                    expr = Expr::FieldAccess {
                        expr: Box::new(expr),
                        field,
                        id,
                        span,
                    };
                }
            } else {
                break;
            }
        }

        expr
    }

    /// Parses a method-call argument list. Behaves like `parse_call_expr`'s
    /// argument loop: positional args are an error; named args are stored
    /// verbatim. The receiver is supplied separately, so this only parses
    /// the comma-separated tail.
    fn parse_method_args(&mut self) -> Vec<NamedArg> {
        let mut args: Vec<NamedArg> = Vec::new();
        if self.check(&TokenKind::RParen) {
            return args;
        }
        loop {
            let arg_start = self.peek().span;
            if self.is_named_arg_start() {
                let name = if let TokenKind::Ident(sym) = self.peek().kind {
                    let sym = sym;
                    self.advance();
                    self.advance();
                    sym
                } else {
                    unreachable!()
                };
                let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                let span = arg_start.to(value.span());
                args.push(NamedArg { span, name, value: Box::new(value) });
            } else {
                self.errors.push(ParseError::PositionalArg { span: arg_start });
                let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                let span = arg_start.to(value.span());
                args.push(NamedArg { span, name: Symbol::new(0), value: Box::new(value) });
            }
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        args
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
            TokenKind::FloatLit(f) => {
                self.advance();
                Expr::Literal {
                    value: Literal::Float(f),
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
                // Variant constructor: `EnumName::Variant(args)` or just
                // `EnumName::Variant` (zero payload).
                if self.check(&TokenKind::ColonColon) {
                    self.advance(); // '::'
                    let variant = match self.peek().kind {
                        TokenKind::Ident(s) => {
                            self.advance();
                            s
                        }
                        _ => {
                            let tok = self.peek().clone();
                            self.errors.push(ParseError::UnexpectedToken {
                                expected: "variant name".to_string(),
                                found: tok.kind,
                                span: tok.span,
                            });
                            return Expr::Literal {
                                value: Literal::Unit,
                                id,
                                span: token.span,
                            };
                        }
                    };
                    let mut args: Vec<Expr> = Vec::new();
                    let mut end_span = self.tokens[self.current - 1].span;
                    if self.match_token(&TokenKind::LParen) {
                        if !self.check(&TokenKind::RParen) {
                            loop {
                                let arg = self.with_struct_literal_allowed(|p| p.parse_expr());
                                args.push(arg);
                                if !self.match_token(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        if let Ok(tok) = self.expect(TokenKind::RParen, "')'") {
                            end_span = tok.span;
                        }
                    }
                    return Expr::VariantCtor {
                        enum_name: name,
                        variant,
                        args,
                        id,
                        span: token.span.to(end_span),
                    };
                }

                // Struct literal: `Name { field: value, ... }`.
                // Only when (a) we're not in a "no-struct-literal" context,
                // and (b) the body is unambiguously a struct literal — i.e.
                // empty `Name {}` or `Name { ident:`.
                if !self.no_struct_literal
                    && self.check(&TokenKind::LBrace)
                    && self.looks_like_struct_lit_body()
                {
                    self.advance(); // '{'
                    let mut fields: Vec<(Symbol, Expr)> = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                        let field_name = match self.peek().kind {
                            TokenKind::Ident(s) => {
                                self.advance();
                                s
                            }
                            _ => {
                                let tok = self.peek().clone();
                                self.errors.push(ParseError::UnexpectedToken {
                                    expected: "field name".to_string(),
                                    found: tok.kind,
                                    span: tok.span,
                                });
                                break;
                            }
                        };
                        if self.expect(TokenKind::Colon, "':'").is_err() {
                            break;
                        }
                        let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                        fields.push((field_name, value));
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
                        tok.span
                    } else {
                        self.peek().span
                    };
                    return Expr::StructLit {
                        name,
                        fields,
                        id,
                        span: token.span.to(end_span),
                    };
                }

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

                // Otherwise, parse the first expression. Could be a
                // parenthesised expression or a tuple.
                let first = self.with_struct_literal_allowed(|p| p.parse_expr());

                if self.check(&TokenKind::Comma) {
                    // Tuple: collect remaining elements.
                    let mut elements = vec![first];
                    while self.match_token(&TokenKind::Comma) {
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                        let next = self.with_struct_literal_allowed(|p| p.parse_expr());
                        elements.push(next);
                    }
                    let end_span = if let Ok(tok) = self.expect(TokenKind::RParen, "')'") {
                        tok.span
                    } else {
                        self.peek().span
                    };
                    return Expr::Tuple {
                        elements,
                        id,
                        span: token.span.to(end_span),
                    };
                }

                if let Err(()) = self.expect(TokenKind::RParen, "')'") {
                    // Error already recorded, return the expression anyway
                }

                first
            }
            TokenKind::LBrace => self.parse_block(),
            // Zero-parameter closure: || { body }
            TokenKind::OrOr => {
                let start_span = token.span;
                self.advance(); // consume '||'
                let body = self.parse_block();
                let span = start_span.to(body.span());
                Expr::Closure {
                    params: vec![],
                    body: Box::new(body),
                    id,
                    span,
                }
            }
            // Shell expression: `$ cmd ...`
            TokenKind::ShellLine(parts) => {
                self.advance();
                let cooked_parts = self.cook_shell_parts(parts);
                Expr::Shell {
                    parts: cooked_parts,
                    id,
                    span: token.span,
                }
            }
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

    /// Parses a require statement.
    ///
    /// Grammar:
    /// ```text
    /// require_stmt ::= "require" ("(" "warn" ")")? expr
    ///                  ("," string_expr)?
    ///                  ("," "set" ":" closure_expr)?
    /// ```
    fn parse_require_stmt(&mut self) -> Option<Stmt> {
        let start_span = self.peek().span;
        self.advance(); // consume 'require'

        // Parse optional mode: (warn)
        let mode = if self.match_token(&TokenKind::LParen) {
            let mode = match &self.peek().kind {
                TokenKind::Ident(_) => {
                    self.advance(); // consume the mode identifier (e.g. "warn")
                    RequireMode::Warn
                }
                _ => {
                    let tok = self.peek().clone();
                    self.errors.push(ParseError::InvalidRequireMode { span: tok.span });
                    // recover: skip to ')'
                    while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                        self.advance();
                    }
                    RequireMode::Error
                }
            };
            self.expect(TokenKind::RParen, "')'").ok();
            mode
        } else {
            RequireMode::Error
        };

        // Parse condition expression
        let expr = Box::new(self.parse_expr());

        // Parse optional message and/or set: closure
        let mut message: Option<Box<Expr>> = None;
        let mut set_fn: Option<Box<Expr>> = None;

        // First optional comma-separated part
        if self.match_token(&TokenKind::Comma) {
            if self.is_set_label_start() {
                // "set" ":" closure_expr
                set_fn = self.parse_set_clause();
            } else if self.is_expr_start() {
                // message expression
                message = Some(Box::new(self.parse_expr()));

                // Second optional comma-separated part (set:)
                if self.match_token(&TokenKind::Comma) {
                    if self.is_set_label_start() {
                        set_fn = self.parse_set_clause();
                    }
                }
            }
        }

        // Optional trailing semicolon
        self.match_token(&TokenKind::Semi);

        let end_span = set_fn.as_ref().map(|e| e.span())
            .or_else(|| message.as_ref().map(|e| e.span()))
            .unwrap_or(expr.span());

        let span = start_span.to(end_span);

        Some(Stmt::Require(RequireStmt {
            span,
            mode,
            expr,
            message,
            set_fn,
        }))
    }

    /// Converts lexer-level shell parts (with sub-token streams) into AST shell
    /// parts (with parsed `Expr` nodes for each interpolation).
    fn cook_shell_parts(&mut self, parts: Vec<ShellTokenPart>) -> Vec<ShellPart> {
        let mut out = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                ShellTokenPart::Literal(s) => out.push(ShellPart::Literal(s)),
                ShellTokenPart::Interpolated(mut sub_tokens) => {
                    // The sub-lexer didn't append an EOF; add one so the
                    // sub-parser can recognise the end of input.
                    let eof_span = sub_tokens
                        .last()
                        .map(|t| ferric_common::Span::new(t.span.end, t.span.end))
                        .unwrap_or_else(|| ferric_common::Span::new(0, 0));
                    sub_tokens.push(Token::new(TokenKind::Eof, eof_span));
                    // Run a fresh sub-parser on the interpolated tokens.
                    let mut sub_parser = Parser::new(&sub_tokens);
                    sub_parser.node_id_gen = NodeIdGen { next: self.node_id_gen.next };
                    let expr = sub_parser.parse_expr();
                    self.node_id_gen.next = sub_parser.node_id_gen.next;
                    self.errors.extend(sub_parser.errors);
                    out.push(ShellPart::Interpolated(Box::new(expr)));
                }
            }
        }
        out
    }

    /// Returns true if the current token stream looks like `ident ':'` (a named label start).
    fn is_set_label_start(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_))
            && self.current + 1 < self.tokens.len()
            && matches!(self.tokens[self.current + 1].kind, TokenKind::Colon)
    }

    /// Parses `set ":" closure_expr` and returns the closure expression.
    fn parse_set_clause(&mut self) -> Option<Box<Expr>> {
        // consume "set" ident and ":"
        self.advance(); // ident ("set")
        self.advance(); // ":"
        // parse closure expression
        if self.is_expr_start() {
            Some(Box::new(self.parse_expr()))
        } else {
            let tok = self.peek().clone();
            self.errors.push(ParseError::ExpectedExpression {
                found: tok.kind,
                span: tok.span,
            });
            None
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

/// Convenience wrapper that runs `parse` and serialises the result as JSON.
///
/// External tools that consume the AST should call `ferric --dump-ast` rather
/// than depending on this crate directly; this helper exists so that consumers
/// inside the workspace (e.g. the CLI) can produce the same output without
/// re-implementing the serialisation step.
pub fn parse_to_json(lex: &LexResult) -> Result<String, serde_json::Error> {
    let result = parse(lex);
    ferric_common::ast_to_json(&result)
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
            _ => panic!("Expected function definition"),
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
                assert_eq!(params[0].name, x);
                assert_eq!(params[1].name, y);
                assert_eq!(*ret_ty, TypeAnnotation::Named(int_ty));
            }
            _ => panic!("Expected function definition"),
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
            _ => panic!("Expected function definition"),
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
        let x = interner.intern("x");
        let y = interner.intern("y");

        // fn foo() { bar(x: 1, y: 2) }
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::Ident(bar), 11, 14),
            tok(TokenKind::LParen, 14, 15),
            tok(TokenKind::Ident(x), 15, 16),
            tok(TokenKind::Colon, 16, 17),
            tok(TokenKind::IntLit(1), 18, 19),
            tok(TokenKind::Comma, 19, 20),
            tok(TokenKind::Ident(y), 21, 22),
            tok(TokenKind::Colon, 22, 23),
            tok(TokenKind::IntLit(2), 24, 25),
            tok(TokenKind::RParen, 25, 26),
            tok(TokenKind::RBrace, 27, 28),
            tok(TokenKind::Eof, 28, 28),
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
                        assert_eq!(args[0].name, x);
                        assert_eq!(args[1].name, y);
                    }
                    _ => panic!("Expected call expression"),
                },
                _ => panic!("Expected block with expression"),
            },
            _ => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_positional_arg_is_error() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        let bar = interner.intern("bar");

        // fn foo() { bar(1) } — positional arg should emit PositionalArg error
        let tokens = vec![
            tok(TokenKind::Fn, 0, 2),
            tok(TokenKind::Ident(foo), 3, 6),
            tok(TokenKind::LParen, 6, 7),
            tok(TokenKind::RParen, 7, 8),
            tok(TokenKind::LBrace, 9, 10),
            tok(TokenKind::Ident(bar), 11, 14),
            tok(TokenKind::LParen, 14, 15),
            tok(TokenKind::IntLit(1), 15, 16),
            tok(TokenKind::RParen, 16, 17),
            tok(TokenKind::RBrace, 18, 19),
            tok(TokenKind::Eof, 19, 19),
        ];

        let lex = make_lex_result(tokens);
        let result = parse(&lex);

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(result.errors[0], ferric_common::ParseError::PositionalArg { .. }));
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
