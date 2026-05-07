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
    AsyncBlockExpr, BinOp, CastExpr, EffectAnnotation, EffectDecl, EffectOp, ExportDecl, Expr,
    FStringExpr, FStringSegment, FStringSegmentKind, FnItem, HandleExpr, HandlerClause, ImplMethod,
    ImportDecl, ImportItem, ImportItems, ImportPath, Interner, Item, Label, LetPattern, LexResult,
    Literal, MatchArm, MustExpr, NamedArg, NodeId, Param, ParseError, ParseResult, ParseWarning,
    Pattern, PerformExpr, PipelineExpr, PropagateExpr, RequireMode, RequireStmt, ResumeExpr,
    ShellPart, ShellTokenPart, Span, Stmt, Symbol, Token, TokenKind, TraitMethod, Ty, TypeAliasItem,
    TypeAnnotation, TypeParam, UnOp,
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

/// Pre-interned symbols for desugaring `async`/`gen` syntax. The parser
/// needs these well-known names to construct `Expr::Perform` and the
/// effect-row return-type annotations without a mutable interner during
/// parsing.
#[derive(Debug, Clone, Copy)]
struct DesugarSyms {
    async_eff: Symbol,
    suspend_op: Symbol,
    future_param: Symbol,
    gen_eff: Symbol,
    yield_op: Symbol,
    value_param: Symbol,
    cont_default: Symbol,
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
    /// Accumulated warnings (non-fatal diagnostics)
    warnings: Vec<ParseWarning>,
    /// Interner — needed to classify `import "..."` path strings at parse time.
    /// Optional so unit tests that construct tokens directly can omit it; when
    /// `None`, the parser cannot classify import paths (they emit a
    /// `ParseError::InvalidImportPath` for safety).
    interner: Option<&'a Interner>,
    /// Pre-interned symbols used by `async`/`gen`/effects desugaring. `None`
    /// when the parser is constructed without an interner (legacy test path);
    /// in that case async/gen desugaring falls back to constructing
    /// AST without proper symbol identity, which downstream stages won't
    /// resolve. This is acceptable for unit tests that construct tokens
    /// directly and don't exercise async/gen forms.
    desugar: Option<DesugarSyms>,
    /// When true, an `Ident { ... }` expression is NOT parsed as a struct
    /// literal — it's left for an outer construct (e.g. `if cond { ... }`,
    /// `while cond { ... }`, `match scrutinee { ... }`) to consume the `{`.
    /// Mirrors rustc's `Restrictions::NO_STRUCT_LITERAL`.
    no_struct_literal: bool,
    /// Nesting count of function-shaped scopes (`fn`, impl methods, closures,
    /// and `async { ... }` blocks). Used as a parse-time fast-path so that
    /// `await` at file top level (depth 0) is rejected immediately. Inside
    /// any function-shaped scope, we defer to the type checker (Task 3).
    fn_depth: u32,
    /// Stack of continuation variable names — pushed on entering a handler
    /// clause body, popped on leaving. `resume k with v` is only legal when
    /// `k` matches the symbol on the top of this stack.
    cont_stack: Vec<Symbol>,
}

impl<'a> Parser<'a> {
    /// Creates a new parser for the given tokens.
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            current: 0,
            node_id_gen: NodeIdGen::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            interner: None,
            desugar: None,
            no_struct_literal: false,
            fn_depth: 0,
            cont_stack: Vec::new(),
        }
    }

    /// Creates a new parser that has access to the interner for path
    /// classification and effect-syntax desugaring. The well-known names
    /// (`Async`, `Suspend`, etc.) used by desugaring must already be present
    /// in `interner`; callers should pre-intern them via
    /// `Interner::pre_intern_desugar_names` before invoking the parser.
    fn new_with_interner(tokens: &'a [Token], interner: &'a Interner) -> Self {
        let desugar = Some(DesugarSyms {
            async_eff: interner.lookup("Async").unwrap_or(Symbol::new(0)),
            suspend_op: interner.lookup("Suspend").unwrap_or(Symbol::new(0)),
            future_param: interner.lookup("future").unwrap_or(Symbol::new(0)),
            gen_eff: interner.lookup("Gen").unwrap_or(Symbol::new(0)),
            yield_op: interner.lookup("Yield").unwrap_or(Symbol::new(0)),
            value_param: interner.lookup("value").unwrap_or(Symbol::new(0)),
            cont_default: interner.lookup("k").unwrap_or(Symbol::new(0)),
        });
        Self {
            tokens,
            current: 0,
            node_id_gen: NodeIdGen::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            interner: Some(interner),
            desugar,
            no_struct_literal: false,
            fn_depth: 0,
            cont_stack: Vec::new(),
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
            self.tokens
                .last()
                .expect("Token stream should not be empty")
        })
    }

    /// Advances to the next token and returns the previous current token.
    fn advance(&mut self) -> &Token {
        let current = self.current;
        if self.current < self.tokens.len() {
            self.current += 1;
        }
        self.tokens.get(current).unwrap_or_else(|| {
            self.tokens
                .last()
                .expect("Token stream should not be empty")
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
    ///
    /// Tracks whether the import section has closed: the first non-import item
    /// closes it, and any subsequent `import` is reported as `LateImport` and
    /// skipped (kept out of the AST so downstream stages don't try to resolve
    /// it).
    fn parse_program(&mut self) -> Vec<Item> {
        let mut items = Vec::new();
        let mut import_section_closed = false;

        while !self.is_at_end() {
            if matches!(self.peek().kind, TokenKind::Import) {
                if import_section_closed {
                    // Skip the late import and emit an error spanning the whole decl.
                    let start_span = self.peek().span;
                    let end_span = self.skip_import_decl();
                    self.errors.push(ParseError::LateImport {
                        span: start_span.to(end_span),
                    });
                    continue;
                }
                if let Some(item) = self.parse_import_decl() {
                    items.push(item);
                } else {
                    self.synchronize();
                }
                continue;
            }

            if let Some(item) = self.parse_item() {
                import_section_closed = true;
                items.push(item);
            } else {
                // Skip to next likely item start on error
                self.synchronize();
            }
        }

        items
    }

    /// Skips tokens making up an `import ... from "..."` declaration after a
    /// `LateImport` is detected. Returns the span of the last consumed token.
    fn skip_import_decl(&mut self) -> Span {
        // Consume tokens until we either consume a string literal (the path)
        // or hit a token that clearly starts a new top-level item.
        let mut last_span = self.peek().span;
        self.advance(); // 'import'
        while !self.is_at_end() {
            last_span = self.peek().span;
            match &self.peek().kind {
                TokenKind::StrLit(_) => {
                    self.advance();
                    return last_span;
                }
                TokenKind::Fn
                | TokenKind::Async
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Trait
                | TokenKind::Impl
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Type
                | TokenKind::Let => return last_span,
                _ => {
                    self.advance();
                }
            }
        }
        last_span
    }

    /// Attempts to synchronize after a parse error by advancing to a likely recovery point.
    fn synchronize(&mut self) {
        self.advance();

        while !self.is_at_end() {
            // Stop at the start of a new item
            if matches!(
                self.peek().kind,
                TokenKind::Fn
                    | TokenKind::Async
                    | TokenKind::Gen
                    | TokenKind::Struct
                    | TokenKind::Enum
                    | TokenKind::Trait
                    | TokenKind::Impl
                    | TokenKind::Import
                    | TokenKind::Export
                    | TokenKind::Type
                    | TokenKind::Effect
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
            TokenKind::Async => self.parse_async_item(),
            TokenKind::Gen => self.parse_gen_item(),
            TokenKind::Effect => self.parse_effect_decl(),
            TokenKind::Struct => self.parse_struct_def(),
            TokenKind::Enum => self.parse_enum_def(),
            TokenKind::Trait => self.parse_trait_def(),
            TokenKind::Impl => self.parse_impl_block(),
            TokenKind::Let => self.parse_script_let(),
            TokenKind::Require => self.parse_script_require(),
            TokenKind::For => self.parse_script_for(),
            // M10 Task 5: labeled `for` at script level — dispatch to the
            // statement variant. `'label: while`/`'label: loop` are
            // expressions and fall through to `parse_script_expr`.
            TokenKind::Label(_) if self.label_then_for() => {
                let stmt = self.parse_labeled_for_stmt()?;
                let span = stmt.span();
                let id = self.node_id_gen.next();
                Some(Item::Script { stmt, id, span })
            }
            TokenKind::Export => self.parse_export_decl(),
            TokenKind::Type => self.parse_type_alias().map(Item::TypeAlias),
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
        Some(Item::StructDef {
            id,
            name,
            fields,
            span,
        })
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
                    while let Some(t) = self.parse_type() {
                        payload.push(t);
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
        Some(Item::EnumDef {
            id,
            name,
            variants,
            span,
        })
    }

    /// Parses a top-level require statement (script mode).
    fn parse_script_require(&mut self) -> Option<Item> {
        let stmt = self.parse_require_stmt()?;
        let span = stmt.span();
        let id = self.node_id_gen.next();
        Some(Item::Script { stmt, id, span })
    }

    /// Parses a top-level for-loop (script mode).
    fn parse_script_for(&mut self) -> Option<Item> {
        let stmt = self.parse_for_stmt()?;
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

    /// Parses an `import` declaration:
    ///
    /// ```text
    /// import_decl ::= "import" import_items "from" string_lit
    /// import_items ::= "{" named_import ("," named_import)* ","? "}"
    ///                | "*" "as" ident
    /// named_import ::= ident ("as" ident)?
    /// ```
    ///
    /// `import X from "path"` (no braces, no `*`) is reported as
    /// `ParseError::DefaultImport` and skipped.
    fn parse_import_decl(&mut self) -> Option<Item> {
        let start_span = self.peek().span;
        self.advance(); // consume 'import'

        // Three cases distinguished by the next token:
        //   `{` → named imports
        //   `*` → namespace import (Star token)
        //   `Ident` → JS-style default import → error
        let items = match self.peek().kind {
            TokenKind::LBrace => {
                self.advance(); // '{'
                let mut imports: Vec<ImportItem> = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    let item_start = self.peek().span;
                    let name = match self.peek().kind {
                        TokenKind::Ident(s) => {
                            self.advance();
                            s
                        }
                        _ => {
                            let tok = self.peek().clone();
                            self.errors.push(ParseError::UnexpectedToken {
                                expected: "imported name".to_string(),
                                found: tok.kind,
                                span: tok.span,
                            });
                            break;
                        }
                    };
                    let mut item_end = self.tokens[self.current - 1].span;
                    let alias = if self.match_token(&TokenKind::As) {
                        match self.peek().kind {
                            TokenKind::Ident(s) => {
                                item_end = self.peek().span;
                                self.advance();
                                Some(s)
                            }
                            _ => {
                                let tok = self.peek().clone();
                                self.errors.push(ParseError::UnexpectedToken {
                                    expected: "alias name after 'as'".to_string(),
                                    found: tok.kind,
                                    span: tok.span,
                                });
                                None
                            }
                        }
                    } else {
                        None
                    };
                    imports.push(ImportItem {
                        span: item_start.to(item_end),
                        name,
                        alias,
                    });
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
                if self.expect(TokenKind::RBrace, "'}'").is_err() {
                    return None;
                }
                ImportItems::Named(imports)
            }
            TokenKind::Star => {
                self.advance(); // '*'
                if self.expect(TokenKind::As, "'as'").is_err() {
                    return None;
                }
                let alias = match self.peek().kind {
                    TokenKind::Ident(s) => {
                        self.advance();
                        s
                    }
                    _ => {
                        let tok = self.peek().clone();
                        self.errors.push(ParseError::UnexpectedToken {
                            expected: "namespace alias".to_string(),
                            found: tok.kind,
                            span: tok.span,
                        });
                        return None;
                    }
                };
                ImportItems::Namespace(alias)
            }
            TokenKind::Ident(_) => {
                // JS-style default import: `import X from "..."`. Report and
                // skip the rest of the declaration so we don't cascade.
                let ident_span = self.peek().span;
                self.errors
                    .push(ParseError::DefaultImport { span: ident_span });
                let _ = self.skip_import_decl_remainder();
                return None;
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "'{', '*', or named imports".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return None;
            }
        };

        if self.expect(TokenKind::From, "'from'").is_err() {
            return None;
        }

        // Path string.
        let (path_str, path_span) = match self.peek().kind {
            TokenKind::StrLit(sym) => {
                let span = self.peek().span;
                self.advance();
                (sym, span)
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "import path string".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return None;
            }
        };

        let path = match self.path_string_from_symbol(path_str) {
            Some(text) => match classify_import_path(&text) {
                Some(p) => p,
                None => {
                    self.errors
                        .push(ParseError::InvalidImportPath { span: path_span });
                    return None;
                }
            },
            // Without an interner we cannot classify; report and skip.
            None => {
                self.errors
                    .push(ParseError::InvalidImportPath { span: path_span });
                return None;
            }
        };

        // Optional trailing semicolon (consistent with other top-level forms).
        self.match_token(&TokenKind::Semi);

        let span = start_span.to(path_span);
        let id = self.node_id_gen.next();
        Some(Item::Import(ImportDecl {
            id,
            span,
            path,
            items,
        }))
    }

    /// After we've already consumed `import` and discovered a malformed shape,
    /// chew through the rest of the declaration up to and including the path
    /// string so the next item starts at a clean boundary.
    fn skip_import_decl_remainder(&mut self) -> Span {
        let mut last_span = self.peek().span;
        while !self.is_at_end() {
            last_span = self.peek().span;
            match &self.peek().kind {
                TokenKind::StrLit(_) => {
                    self.advance();
                    return last_span;
                }
                TokenKind::Fn
                | TokenKind::Async
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Trait
                | TokenKind::Impl
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Type
                | TokenKind::Let => return last_span,
                _ => {
                    self.advance();
                }
            }
        }
        last_span
    }

    /// Resolves a string-literal symbol to its interned text. Returns `None`
    /// if the parser was constructed without an interner (test-only path).
    fn path_string_from_symbol(&self, sym: Symbol) -> Option<String> {
        self.interner.map(|i| i.resolve(sym).to_string())
    }
}

/// Classifies the shape of an import path string:
/// - `./...` or `../...` → `ImportPath::Relative`
/// - `@/...`            → `ImportPath::Workspace`
/// - bare identifier-ish → `ImportPath::Cache`
/// - anything else      → `None` (caller emits `InvalidImportPath`)
fn classify_import_path(s: &str) -> Option<ImportPath> {
    if s.starts_with("./") || s.starts_with("../") {
        Some(ImportPath::Relative(s.to_string()))
    } else if let Some(rest) = s.strip_prefix("@/") {
        if rest.is_empty() {
            None
        } else {
            Some(ImportPath::Workspace(s.to_string()))
        }
    } else if !s.is_empty() && is_valid_cache_name(s) {
        Some(ImportPath::Cache(s.to_string()))
    } else {
        None
    }
}

/// Returns true if `s` looks like a bare cache dependency name: a non-empty
/// string containing only ASCII alphanumerics, `_`, or `-`. No path separators
/// are allowed (those would belong to a relative or workspace path shape).
fn is_valid_cache_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl<'a> Parser<'a> {
    /// Parses an `export` declaration: `export <item>` where `<item>` is one of
    /// `fn`, `struct`, `enum`, or `type` alias. Wraps the parsed inner item
    /// in `ExportDecl`. Anything else is reported as `InvalidExportPosition`.
    fn parse_export_decl(&mut self) -> Option<Item> {
        let start_span = self.peek().span;
        self.advance(); // consume 'export'

        let inner = match self.peek().kind {
            TokenKind::Fn => self.parse_fn_def(),
            TokenKind::Struct => self.parse_struct_def(),
            TokenKind::Enum => self.parse_enum_def(),
            TokenKind::Type => self.parse_type_alias().map(Item::TypeAlias),
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::InvalidExportPosition {
                    span: start_span.to(tok.span),
                });
                return None;
            }
        };
        let inner = inner?;
        let span = start_span.to(inner.span());
        let id = self.node_id_gen.next();
        Some(Item::Export(ExportDecl {
            id,
            span,
            item: Box::new(inner),
        }))
    }

    /// Parses a `type` alias definition:
    ///
    /// ```text
    /// type_alias ::= "type" ident ("<" ident ("," ident)* ">")? "=" type_expr
    /// ```
    fn parse_type_alias(&mut self) -> Option<TypeAliasItem> {
        let start_span = self.peek().span;
        self.advance(); // consume 'type'

        let name = match self.peek().kind {
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "type alias name".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return None;
            }
        };

        // Optional generic parameter list: `<T1, T2>`. Bounds are not allowed
        // on type aliases in M7, so we accept identifiers only.
        let mut params: Vec<Symbol> = Vec::new();
        if self.match_token(&TokenKind::Lt) {
            if !self.check(&TokenKind::Gt) {
                loop {
                    match self.peek().kind {
                        TokenKind::Ident(s) => {
                            self.advance();
                            params.push(s);
                        }
                        _ => {
                            let tok = self.peek().clone();
                            self.errors.push(ParseError::UnexpectedToken {
                                expected: "type parameter name".to_string(),
                                found: tok.kind,
                                span: tok.span,
                            });
                            break;
                        }
                    }
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            let _ = self.expect(TokenKind::Gt, "'>'");
        }

        if self.expect(TokenKind::Eq, "'='").is_err() {
            return None;
        }

        let ty = self.parse_type()?;
        let end_span = self.tokens[self.current - 1].span;

        // Optional trailing semicolon.
        self.match_token(&TokenKind::Semi);

        let span = start_span.to(end_span);
        let id = self.node_id_gen.next();
        Some(TypeAliasItem {
            id,
            span,
            name,
            params,
            ty,
            opaque: true,
        })
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

                let param_span =
                    param_start.to(default.as_ref().map(|d| d.span()).unwrap_or(ty_end_span));
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

        // Parse body (must be a block). Bumping `fn_depth` here is what
        // tells the prefix `await` parser to defer to the type checker
        // for await-context validation.
        let body = if self.check(&TokenKind::LBrace) {
            self.fn_depth += 1;
            let body = self.parse_block();
            self.fn_depth -= 1;
            body
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

        Some(Item::Fn(FnItem {
            id,
            name,
            type_params,
            params,
            ret_ty,
            body,
            span,
        }))
    }

    /// Parses an `async`-prefixed item. Currently the only legal form at the
    /// item level is `async fn ...`; anything else (e.g. `async let`,
    /// `async struct`) produces `ParseError::AsyncOnNonFn`.
    ///
    /// **M9 desugaring (parse-time):** `async fn name(params) -> T { body }`
    /// is rewritten to a plain `Item::Fn` whose return type is
    /// `Eff<[Async], T>`. Every `await expr` in the body is rewritten to
    /// `perform Async::Suspend(future: expr)` by the `await` parser arm —
    /// this function only rewrites the wrapping fn shape.
    ///
    /// `async { ... }` blocks are expressions, not items, and never reach
    /// here — they are handled in `parse_primary` via `parse_script_expr`.
    fn parse_async_item(&mut self) -> Option<Item> {
        let async_span = self.peek().span;
        self.advance(); // consume 'async'

        match self.peek().kind {
            TokenKind::Fn => {
                let inner = self.parse_fn_def()?;
                let mut item = match inner {
                    Item::Fn(item) => item,
                    other => return Some(other), // parse_fn_def always returns Item::Fn — be defensive
                };
                // Desugar: wrap the return type in `Eff<[Async], T>`.
                if let Some(syms) = self.desugar {
                    let inner_ret = std::mem::replace(&mut item.ret_ty, TypeAnnotation::Infer);
                    item.ret_ty = TypeAnnotation::Eff {
                        row: vec![EffectAnnotation {
                            name: syms.async_eff,
                            args: vec![],
                        }],
                        result: Box::new(inner_ret),
                    };
                }
                item.span = async_span.to(item.span);
                Some(Item::Fn(item))
            }
            TokenKind::LBrace => {
                // `async {` at the top level: parse the async block as a
                // script expression so the user gets the natural surface
                // syntax `async { ... }` at file scope. Push the parser
                // back one token so `parse_script_expr` re-enters from the
                // `Async` token and dispatches into `parse_primary`.
                self.current -= 1;
                self.parse_script_expr()
            }
            _ => {
                let bad = self.peek().clone();
                let span = async_span.to(bad.span);
                self.errors.push(ParseError::AsyncOnNonFn { span });
                // Recover: skip the offending modifier and continue parsing
                // whatever follows as a normal item.
                self.parse_item()
            }
        }
    }

    /// Parses a `gen`-prefixed item. Only `gen fn ...` is legal.
    ///
    /// **M9 desugaring (parse-time):** `gen fn name(params) -> T { body }`
    /// is rewritten to a plain `Item::Fn` whose return type is
    /// `Eff<[Gen<T>], Unit>`. Every `yield expr` in the body is rewritten
    /// to `perform Gen::Yield(value: expr)` by the `yield` parser arm.
    fn parse_gen_item(&mut self) -> Option<Item> {
        let gen_span = self.peek().span;
        self.advance(); // consume 'gen'

        if !matches!(self.peek().kind, TokenKind::Fn) {
            let bad = self.peek().clone();
            let span = gen_span.to(bad.span);
            self.errors.push(ParseError::UnexpectedToken {
                expected: "`fn` after `gen`".to_string(),
                found: bad.kind,
                span,
            });
            return self.parse_item();
        }

        let inner = self.parse_fn_def()?;
        let mut item = match inner {
            Item::Fn(item) => item,
            other => return Some(other),
        };

        // Desugar: wrap the return type as `Eff<[Gen<T>], Unit>`.
        // The inner item's `ret_ty` becomes the type argument to `Gen<...>`.
        if let Some(syms) = self.desugar {
            let inner_ret = std::mem::replace(&mut item.ret_ty, TypeAnnotation::Infer);
            // The "Unit" name is special — represented as `TypeAnnotation::Named`
            // with the symbol for "Unit" if we have it interned, otherwise a
            // sentinel. The downstream resolver treats Symbol(0)→"Unit" but
            // safer to look it up.
            let unit_sym = self
                .interner
                .and_then(|i| i.lookup("Unit"))
                .unwrap_or(Symbol::new(0));
            item.ret_ty = TypeAnnotation::Eff {
                row: vec![EffectAnnotation {
                    name: syms.gen_eff,
                    args: vec![inner_ret],
                }],
                result: Box::new(TypeAnnotation::Named(unit_sym)),
            };
        }
        item.span = gen_span.to(item.span);
        Some(Item::Fn(item))
    }

    /// Parses an `effect` declaration:
    ///
    /// ```text
    /// effect Name<TypeParams>? {
    ///     OpName(param: Type, ...) -> ResumeType,
    ///     ...
    /// }
    /// ```
    ///
    /// Effect declarations are only legal at module scope; the block-level
    /// parser emits `ParseError::EffectDeclInsideFn` if one appears inside a
    /// function body.
    fn parse_effect_decl(&mut self) -> Option<Item> {
        let start_span = self.peek().span;
        self.advance(); // consume 'effect'

        let name = match self.peek().kind {
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "effect name".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return None;
            }
        };

        // Optional generic type parameters: `<T, U>`. Effect-level type
        // params are simple symbols (no bounds in M9).
        let mut type_params: Vec<Symbol> = Vec::new();
        if self.match_token(&TokenKind::Lt) {
            while !self.check(&TokenKind::Gt) && !self.is_at_end() {
                let tp_name = match self.peek().kind {
                    TokenKind::Ident(s) => {
                        self.advance();
                        s
                    }
                    _ => {
                        let tok = self.peek().clone();
                        self.errors.push(ParseError::UnexpectedToken {
                            expected: "type parameter name".to_string(),
                            found: tok.kind,
                            span: tok.span,
                        });
                        break;
                    }
                };
                type_params.push(tp_name);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            let _ = self.expect(TokenKind::Gt, "'>'");
        }

        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            return None;
        }

        let mut ops: Vec<EffectOp> = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let op_start = self.peek().span;
            let op_name = match self.peek().kind {
                TokenKind::Ident(s) => {
                    self.advance();
                    s
                }
                _ => {
                    let tok = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "effect operation name".to_string(),
                        found: tok.kind,
                        span: tok.span,
                    });
                    break;
                }
            };

            if self.expect(TokenKind::LParen, "'('").is_err() {
                break;
            }

            let mut params: Vec<(Symbol, Ty)> = Vec::new();
            if !self.check(&TokenKind::RParen) {
                loop {
                    let pname = match self.peek().kind {
                        TokenKind::Ident(s) => {
                            self.advance();
                            s
                        }
                        _ => {
                            let tok = self.peek().clone();
                            self.errors.push(ParseError::UnexpectedToken {
                                expected: "parameter name".to_string(),
                                found: tok.kind,
                                span: tok.span,
                            });
                            break;
                        }
                    };
                    if self.expect(TokenKind::Colon, "':'").is_err() {
                        break;
                    }
                    let pty_ann = match self.parse_type() {
                        Some(t) => t,
                        None => break,
                    };
                    let pty = self.lower_type_annotation(&pty_ann);
                    params.push((pname, pty));
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            let _ = self.expect(TokenKind::RParen, "')'");

            // Resume type: `-> Type` (required).
            let resume_type = if self.match_token(&TokenKind::Arrow) {
                match self.parse_type() {
                    Some(t) => self.lower_type_annotation(&t),
                    None => Ty::Unit,
                }
            } else {
                Ty::Unit
            };

            let op_end = self.tokens[self.current.saturating_sub(1)].span;
            ops.push(EffectOp {
                span: op_start.to(op_end),
                name: op_name,
                params,
                resume_type,
            });

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
            tok.span
        } else {
            self.peek().span
        };

        Some(Item::EffectDecl(EffectDecl {
            span: start_span.to(end_span),
            name,
            type_params,
            ops,
        }))
    }

    /// Lowers a `TypeAnnotation` into a concrete `Ty` for use in
    /// `EffectOp::params`/`resume_type`. The parser produces a syntactic
    /// approximation; the typechecker will refine effect-decl types in
    /// Task 3. For now this maps the well-known primitives and falls back
    /// to a `Ty::Unknown`-style sentinel via `Ty::Var(0)`.
    fn lower_type_annotation(&self, ann: &TypeAnnotation) -> Ty {
        match ann {
            TypeAnnotation::Named(sym) => {
                let name = self
                    .interner
                    .map(|i| i.resolve(*sym).to_string())
                    .unwrap_or_default();
                match name.as_str() {
                    "Int" => Ty::Int,
                    "Float" => Ty::Float,
                    "Bool" => Ty::Bool,
                    "Str" => Ty::Str,
                    "Unit" | "" => Ty::Unit,
                    "ShellOutput" => Ty::ShellOutput,
                    _ => Ty::Var(ferric_common::TyVar(0)),
                }
            }
            TypeAnnotation::Array(inner) => Ty::Array(Box::new(self.lower_type_annotation(inner))),
            TypeAnnotation::Generic { head, args } => {
                let name = self
                    .interner
                    .map(|i| i.resolve(*head).to_string())
                    .unwrap_or_default();
                match (name.as_str(), args.as_slice()) {
                    ("Option", [t]) => Ty::Option(Box::new(self.lower_type_annotation(t))),
                    ("Result", [t, e]) => Ty::Result(
                        Box::new(self.lower_type_annotation(t)),
                        Box::new(self.lower_type_annotation(e)),
                    ),
                    ("Async", [t]) => Ty::Async(Box::new(self.lower_type_annotation(t))),
                    ("Handle", [t]) => Ty::Handle(Box::new(self.lower_type_annotation(t))),
                    ("Poll", [t]) => Ty::Poll(Box::new(self.lower_type_annotation(t))),
                    _ => Ty::Var(ferric_common::TyVar(0)),
                }
            }
            TypeAnnotation::Eff { row, result } => {
                let effects = row
                    .iter()
                    .map(|e| ferric_common::EffectRef {
                        name: e.name,
                        args: e
                            .args
                            .iter()
                            .map(|a| self.lower_type_annotation(a))
                            .collect(),
                    })
                    .collect();
                Ty::Eff(effects, Box::new(self.lower_type_annotation(result)))
            }
            TypeAnnotation::Infer => Ty::Var(ferric_common::TyVar(0)),
        }
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
            let _ = self.match_token(&TokenKind::Semi) || self.match_token(&TokenKind::Comma);

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
        Some(Item::TraitDef {
            id,
            name,
            methods,
            span,
        })
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
    /// represented by an `ImplMethod`, not `Item::Fn`.
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
            self.fn_depth += 1;
            let body = self.parse_block();
            self.fn_depth -= 1;
            body
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
                let head = *sym;
                self.advance();

                // Generic type application: `Name<T1, T2, ...>`. The head's
                // identity (Option, Result, …) is resolved by later stages
                // that have access to the interner.
                if self.check(&TokenKind::Lt) {
                    self.advance(); // '<'
                    let mut args: Vec<TypeAnnotation> = Vec::new();
                    if !self.check(&TokenKind::Gt) {
                        loop {
                            let arg = self.parse_type()?;
                            args.push(arg);
                            if !self.match_token(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    let _ = self.expect(TokenKind::Gt, "'>'");
                    return Some(TypeAnnotation::Generic { head, args });
                }

                Some(TypeAnnotation::Named(head))
            }
            TokenKind::LBracket => {
                // Array type: `[T]`.
                self.advance(); // '['
                let inner = self.parse_type()?;
                let _ = self.expect(TokenKind::RBracket, "']'");
                Some(TypeAnnotation::Array(Box::new(inner)))
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

            // Check for a `for x in iter { body }` loop.
            if self.check(&TokenKind::For) {
                if let Some(stmt) = self.parse_for_stmt() {
                    stmts.push(stmt);
                }
                continue;
            }

            // M10 Task 5: a label followed by `:` `for` is a labeled `for`
            // statement. Other label-prefixed loops (`while`, `loop`) are
            // expressions and are handled by `parse_expr` further below.
            if self.label_then_for() {
                if let Some(stmt) = self.parse_labeled_for_stmt() {
                    stmts.push(stmt);
                }
                continue;
            }

            // `export`, `import`, `type` are top-level only — emit a clear
            // error if they appear inside a block, then skip the keyword and
            // continue parsing the inner thing as if the modifier weren't
            // there. This is more useful than letting the user see a generic
            // UnexpectedToken.
            if self.check(&TokenKind::Export) {
                let span = self.peek().span;
                self.errors.push(ParseError::InvalidExportPosition { span });
                self.advance();
                continue;
            }
            if self.check(&TokenKind::Import) {
                let span = self.peek().span;
                self.errors.push(ParseError::LateImport { span });
                let _ = self.skip_import_decl_remainder();
                continue;
            }
            // `effect` declarations are only legal at module scope. Skip the
            // entire effect declaration on recovery so we don't cascade.
            if self.check(&TokenKind::Effect) {
                let span = self.peek().span;
                self.errors.push(ParseError::EffectDeclInsideFn { span });
                let _ = self.parse_effect_decl();
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
                | TokenKind::OrOr      // zero-param closure: || ...
                | TokenKind::Pipe      // closure: |x| ...
                | TokenKind::LBracket  // array literal: [1, 2]
                | TokenKind::ShellLine(_)
                | TokenKind::Async     // async { ... } block expression
                | TokenKind::Await     // prefix `await expr`
                | TokenKind::Yield     // prefix `yield expr`
                | TokenKind::Perform   // `perform Effect::Op(...)`
                | TokenKind::Handle    // `handle { ... } with { ... }`
                | TokenKind::Resume    // `resume k with v`
                | TokenKind::FStringStart // `f"..."` interpolated string
                | TokenKind::Label(_)  // labeled loop: `'outer: while ...`
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

    /// Parses `for var in iter_expr { body }`.
    ///
    /// `in` is not a reserved keyword — it's lexed as `Ident`. We accept any
    /// identifier whose interned name will later be `"in"`; mismatches produce
    /// an UnexpectedToken error.
    ///
    /// `label` is set when the `for` was preceded by `'name:` (M10 Task 5).
    fn parse_for_stmt_with_label(&mut self, label: Option<Label>) -> Option<Stmt> {
        let kw_span = self.peek().span;
        let start_span = label.as_ref().map(|l| l.span).unwrap_or(kw_span);
        self.advance(); // consume 'for'

        let var_id = self.node_id_gen.next();
        let var = match self.peek().kind {
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "loop variable name".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return None;
            }
        };

        // Expect `in` (an identifier with that text — checked downstream).
        match self.peek().kind {
            TokenKind::Ident(_) => {
                self.advance();
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "'in'".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return None;
            }
        }

        // Parse iterable in a no-struct-literal context so `for x in arr {`
        // doesn't try to consume the body brace as a struct literal.
        let iter = self.with_no_struct_literal(|p| p.parse_expr());

        // Body is a block.
        if !self.check(&TokenKind::LBrace) {
            let tok = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "'{' to start for-loop body".to_string(),
                found: tok.kind,
                span: tok.span,
            });
            return None;
        }
        let body = self.parse_block();
        let span = start_span.to(body.span());
        let id = self.node_id_gen.next();

        Some(Stmt::For {
            var,
            var_id,
            iter,
            body,
            label,
            id,
            span,
        })
    }

    /// Convenience wrapper: parses a non-labeled `for` loop. The caller has
    /// peeked `Token::For` at the current position.
    fn parse_for_stmt(&mut self) -> Option<Stmt> {
        self.parse_for_stmt_with_label(None)
    }

    /// Returns true when the next three tokens are `Label`, `:`, `for`.
    /// Used to dispatch to `parse_labeled_for_stmt` at statement positions.
    fn label_then_for(&self) -> bool {
        if !matches!(self.peek().kind, TokenKind::Label(_)) {
            return false;
        }
        if self.current + 1 >= self.tokens.len()
            || !matches!(self.tokens[self.current + 1].kind, TokenKind::Colon)
        {
            return false;
        }
        if self.current + 2 >= self.tokens.len() {
            return false;
        }
        matches!(self.tokens[self.current + 2].kind, TokenKind::For)
    }

    /// Parses a labeled `for` statement: `'label: for x in iter { body }`.
    /// The caller has verified `label_then_for()`.
    fn parse_labeled_for_stmt(&mut self) -> Option<Stmt> {
        let lab_tok = self.peek().clone();
        let (name, lab_span) = match lab_tok.kind {
            TokenKind::Label(s) => (s, lab_tok.span),
            _ => unreachable!("parse_labeled_for_stmt called without Label token"),
        };
        self.advance(); // consume label
        if self.expect(TokenKind::Colon, "':' after loop label").is_err() {
            return None;
        }
        self.parse_for_stmt_with_label(Some(Label {
            span: lab_span,
            name,
        }))
    }

    /// Parses a let statement: `let name: type = expr;` or `let mut name: type = expr;`.
    ///
    /// M10 Task 5: also handles destructuring patterns:
    /// - `let (a, b, c) = expr` — tuple destructuring (`LetPattern::Tuple`).
    /// - `let Point { x, y } = expr` — struct destructuring with bare-name
    ///   field shorthand (`LetPattern::Struct`).
    /// - `let Foo::Bar(...) = expr` — refutable enum patterns are rejected
    ///   with `ParseError::DestructureIrrefutable`.
    fn parse_let_stmt(&mut self) -> Option<Stmt> {
        let start_span = self.peek().span;
        self.advance(); // consume 'let'

        // Check for 'mut' keyword
        let mutable = self.match_token(&TokenKind::Mut);

        let pattern = self.parse_let_pattern()?;

        // Parse optional type annotation. Currently only legal for ident
        // patterns; for destructuring patterns we accept it without
        // checking — downstream stages infer from the RHS.
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
            pattern,
            mutable,
            ty,
            init,
            id,
            span,
        })
    }

    /// Parses the LHS pattern of a `let`. M10 Task 5.
    fn parse_let_pattern(&mut self) -> Option<LetPattern> {
        match self.peek().kind {
            TokenKind::LParen => self.parse_let_tuple_pattern(),
            TokenKind::Ident(sym) => {
                // Look ahead: an identifier followed by `{` is a struct
                // destructuring pattern. An identifier followed by `::` is a
                // refutable enum pattern — emit DestructureIrrefutable.
                // Anything else is an ident binding.
                let next_kind = self
                    .tokens
                    .get(self.current + 1)
                    .map(|t| &t.kind)
                    .cloned();
                match next_kind {
                    Some(TokenKind::LBrace) => self.parse_let_struct_pattern(sym),
                    Some(TokenKind::ColonColon) => {
                        // Enum pattern: `let Foo::Bar(...) = ...` is refutable.
                        let start_span = self.peek().span;
                        // Consume the (likely) full enum-pattern shape so we
                        // can produce a faithful `pattern` string for the
                        // diagnostic and recover the parser state.
                        let pretty = self.consume_refutable_pattern_text();
                        let end_span = self
                            .tokens
                            .get(self.current.saturating_sub(1))
                            .map(|t| t.span)
                            .unwrap_or(start_span);
                        self.errors.push(ParseError::DestructureIrrefutable {
                            pattern: pretty,
                            span: start_span.to(end_span),
                        });
                        // Recovery: bind a sentinel name so downstream stages
                        // do not cascade. Use a bogus interned name; the
                        // subsequent `=` requirement still applies.
                        Some(LetPattern::Ident(sym))
                    }
                    _ => {
                        // Plain ident binding.
                        self.advance();
                        Some(LetPattern::Ident(sym))
                    }
                }
            }
            _ => {
                let token = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "variable name or destructuring pattern".to_string(),
                    found: token.kind,
                    span: token.span,
                });
                None
            }
        }
    }

    /// Parses `(a, b, c)` as `LetPattern::Tuple`. Caller has peeked `(`.
    fn parse_let_tuple_pattern(&mut self) -> Option<LetPattern> {
        self.advance(); // consume '('
        let mut idents: Vec<Symbol> = Vec::new();
        loop {
            if self.check(&TokenKind::RParen) {
                break;
            }
            match self.peek().kind {
                TokenKind::Ident(s) => {
                    self.advance();
                    idents.push(s);
                }
                _ => {
                    let token = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "identifier in tuple destructuring pattern".to_string(),
                        found: token.kind,
                        span: token.span,
                    });
                    return None;
                }
            }
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        if self.expect(TokenKind::RParen, "')'").is_err() {
            return None;
        }
        Some(LetPattern::Tuple(idents))
    }

    /// Parses `Name { f1, f2 }` as `LetPattern::Struct`. Caller has peeked
    /// the struct name; this function consumes it and the brace block.
    fn parse_let_struct_pattern(&mut self, name: Symbol) -> Option<LetPattern> {
        self.advance(); // consume struct name ident
        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            return None;
        }
        let mut fields: Vec<Symbol> = Vec::new();
        loop {
            if self.check(&TokenKind::RBrace) {
                break;
            }
            match self.peek().kind {
                TokenKind::Ident(s) => {
                    self.advance();
                    // Reject explicit `field: rename` — out of scope for M10
                    // Task 5. (The spec lists this as a deliberately deferred
                    // feature.)
                    if self.check(&TokenKind::Colon) {
                        let token = self.peek().clone();
                        self.errors.push(ParseError::UnexpectedToken {
                            expected: "',' or '}' (field rename `name: alias` is not supported)"
                                .to_string(),
                            found: token.kind,
                            span: token.span,
                        });
                        return None;
                    }
                    fields.push(s);
                }
                _ => {
                    let token = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "field name in struct destructuring pattern".to_string(),
                        found: token.kind,
                        span: token.span,
                    });
                    return None;
                }
            }
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        if self.expect(TokenKind::RBrace, "'}'").is_err() {
            return None;
        }
        Some(LetPattern::Struct { name, fields })
    }

    /// Best-effort textual capture of a refutable pattern at the start of a
    /// `let` LHS. Walks tokens up to the next `=` or end-of-statement and
    /// builds a printable representation. Used solely for diagnostics — the
    /// parser then discards the consumed tokens.
    fn consume_refutable_pattern_text(&mut self) -> String {
        let mut s = String::new();
        let mut depth: i32 = 0;
        while !self.is_at_end() {
            match &self.peek().kind {
                TokenKind::Eq if depth == 0 => break,
                TokenKind::Semi if depth == 0 => break,
                TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => {
                    depth += 1;
                    s.push(match self.peek().kind {
                        TokenKind::LParen => '(',
                        TokenKind::LBrace => '{',
                        _ => '[',
                    });
                }
                TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket => {
                    depth -= 1;
                    s.push(match self.peek().kind {
                        TokenKind::RParen => ')',
                        TokenKind::RBrace => '}',
                        _ => ']',
                    });
                }
                TokenKind::Ident(sym) => {
                    if !s.is_empty() && !s.ends_with('(') && !s.ends_with('{') {
                        s.push(' ');
                    }
                    if let Some(interner) = self.interner {
                        s.push_str(interner.resolve(*sym));
                    } else {
                        s.push_str("<ident>");
                    }
                }
                TokenKind::ColonColon => s.push_str("::"),
                TokenKind::Comma => s.push_str(", "),
                _ => break,
            }
            self.advance();
        }
        s
    }

    /// Parses an expression.
    fn parse_expr(&mut self) -> Expr {
        // Handle control flow keywords as special cases
        match self.peek().kind {
            TokenKind::Return => self.parse_return_expr(),
            TokenKind::If => self.parse_if_expr(),
            TokenKind::While => self.parse_while_expr(None),
            TokenKind::Loop => self.parse_loop_expr(None),
            TokenKind::Break => self.parse_break_expr(),
            TokenKind::Continue => self.parse_continue_expr(),
            TokenKind::Match => self.parse_match_expr(),
            // M10 Task 5: `'label: while|loop ...` — the parser sees a
            // `Token::Label` followed by `:` and a loop keyword. `for` is
            // statement-level only and is handled at the block/script
            // dispatch sites.
            TokenKind::Label(_) => self.parse_labeled_loop_expr(),
            _ => self.parse_pipeline_expr(),
        }
    }

    /// Parses a labeled loop expression: `'name: while ...` or `'name: loop ...`.
    /// The caller must have peeked a `TokenKind::Label`. If the label is not
    /// followed by `:` and a loop keyword, an error is emitted and a unit
    /// literal is returned for recovery. Labeled `for` is a statement, not an
    /// expression — see `parse_labeled_for_stmt`.
    fn parse_labeled_loop_expr(&mut self) -> Expr {
        let label_token = self.peek().clone();
        let (label_name, label_span) = match label_token.kind {
            TokenKind::Label(s) => (s, label_token.span),
            _ => unreachable!("parse_labeled_loop_expr called without a Label token"),
        };
        self.advance(); // consume label
        let label = Some(Label {
            span: label_span,
            name: label_name,
        });
        // Expect `:` after the label.
        if self.expect(TokenKind::Colon, "':' after loop label").is_err() {
            let id = self.node_id_gen.next();
            return Expr::Literal {
                value: Literal::Unit,
                id,
                span: label_span,
            };
        }
        match self.peek().kind {
            TokenKind::While => self.parse_while_expr(label),
            TokenKind::Loop => self.parse_loop_expr(label),
            TokenKind::For => {
                // Labeled `for` is a statement, but we got here from an
                // expression context. Surface a clear error and recover by
                // consuming the rest of the for-loop. The block- or script-level
                // dispatch is responsible for picking up `Label : For`.
                let bad = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "labeled `for` is a statement; use it where statements are allowed"
                        .to_string(),
                    found: bad.kind,
                    span: bad.span,
                });
                let id = self.node_id_gen.next();
                Expr::Literal {
                    value: Literal::Unit,
                    id,
                    span: label_span,
                }
            }
            _ => {
                let bad = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "loop keyword (`while`, `loop`, or `for`) after label".to_string(),
                    found: bad.kind,
                    span: bad.span,
                });
                let id = self.node_id_gen.next();
                Expr::Literal {
                    value: Literal::Unit,
                    id,
                    span: label_span,
                }
            }
        }
    }

    /// Parses pipeline expressions: `lhs |> rhs (|> rhs)*`. M10 (Task 3).
    ///
    /// `|>` is the lowest-precedence binary operator — strictly below `||`.
    /// It is left-associative: `a |> f() |> g()` parses as `(a |> f()) |> g()`.
    /// Each segment is parsed at full binary-expression precedence so binary
    /// operators bind tighter inside both LHS and RHS sub-expressions.
    ///
    /// The parser does not require the RHS to syntactically be a call — that
    /// validation is deferred to the resolver, which emits
    /// `ResolveError::PipelineRhsNotCall` for invalid forms.
    fn parse_pipeline_expr(&mut self) -> Expr {
        let mut left = self.parse_binary_expr(0);
        while matches!(self.peek().kind, TokenKind::Pipe2) {
            self.advance(); // consume `|>`
            let right = self.parse_binary_expr(0);
            let span = left.span().to(right.span());
            left = Expr::Pipeline(PipelineExpr {
                span,
                lhs: Box::new(left),
                rhs: Box::new(right),
            });
        }
        left
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

    /// Parses a while loop: `while cond block`. `label` is set when the loop
    /// was preceded by `'name:` (M10 Task 5).
    fn parse_while_expr(&mut self, label: Option<Label>) -> Expr {
        let kw_span = self.peek().span;
        let start_span = label.as_ref().map(|l| l.span).unwrap_or(kw_span);
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

        Expr::While {
            cond,
            body,
            label,
            id,
            span,
        }
    }

    /// Parses an infinite loop: `loop block`. `label` is set when the loop
    /// was preceded by `'name:` (M10 Task 5).
    fn parse_loop_expr(&mut self, label: Option<Label>) -> Expr {
        let kw_span = self.peek().span;
        let start_span = label.as_ref().map(|l| l.span).unwrap_or(kw_span);
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

        Expr::Loop {
            body,
            label,
            id,
            span,
        }
    }

    /// Parses a break expression: `break` or `break 'label` (M10 Task 5).
    fn parse_break_expr(&mut self) -> Expr {
        let kw_span = self.peek().span;
        self.advance(); // consume 'break'
        // Optional `'label` immediately following.
        let (label, end_span) = if let TokenKind::Label(sym) = self.peek().kind {
            let lab_span = self.peek().span;
            self.advance();
            (
                Some(Label {
                    span: lab_span,
                    name: sym,
                }),
                lab_span,
            )
        } else {
            (None, kw_span)
        };
        let span = kw_span.to(end_span);
        let id = self.node_id_gen.next();

        Expr::Break { label, id, span }
    }

    /// Parses a continue expression: `continue` or `continue 'label` (M10 Task 5).
    fn parse_continue_expr(&mut self) -> Expr {
        let kw_span = self.peek().span;
        self.advance(); // consume 'continue'
        let (label, end_span) = if let TokenKind::Label(sym) = self.peek().kind {
            let lab_span = self.peek().span;
            self.advance();
            (
                Some(Label {
                    span: lab_span,
                    name: sym,
                }),
                lab_span,
            )
        } else {
            (None, kw_span)
        };
        let span = kw_span.to(end_span);
        let id = self.node_id_gen.next();

        Expr::Continue { label, id, span }
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
            arms.push(MatchArm {
                pattern,
                body,
                span,
            });

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
        Expr::Match {
            scrutinee,
            arms,
            id,
            span,
        }
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
        let mut left = self.parse_cast_expr();

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

    /// Parses a cast expression: `expr as TypeExpr`. Cast binds tighter than
    /// every binary operator, but looser than unary (`-`, `!`) and looser than
    /// any postfix operator parsed inside `parse_unary_expr` (field access,
    /// indexing, call). Chained casts (`x as A as B`) are an error
    /// (`ParseError::ChainedCast`).
    fn parse_cast_expr(&mut self) -> Expr {
        let mut expr = self.parse_unary_expr();
        // M10 Task 4: postfix `must` (unwrap). Binds at unary precedence —
        // tighter than any binary operator, looser than postfix call/field.
        //
        // **Message-form ambiguity.** The spec allows a bare `must` (no
        // message) and `must <str_expr>`. Without significant newlines or a
        // mandatory statement terminator we cannot tell those apart in
        // general — a greedy `must` would always swallow the next statement.
        //
        // Resolution: the optional message expression is restricted to a
        // small, syntactically obvious starter set — a string literal, an
        // f-string, or a parenthesised expression — and is parsed at unary
        // precedence so `a must "msg" + tail` still parses as
        // `(a must "msg") + tail`. Any other `Str` value can be wrapped in
        // parens: `x must (some_str_var)`.
        while matches!(self.peek().kind, TokenKind::Must) {
            let must_tok_span = self.peek().span;
            self.advance(); // consume `must`
            let starts_message = matches!(
                self.peek().kind,
                TokenKind::StrLit(_) | TokenKind::FStringStart | TokenKind::LParen
            );
            let message = if starts_message {
                Some(Box::new(self.parse_unary_expr()))
            } else {
                None
            };
            let end_span = match &message {
                Some(m) => m.span(),
                None => must_tok_span,
            };
            let span = expr.span().to(end_span);
            expr = Expr::Must(MustExpr {
                span,
                operand: Box::new(expr),
                message,
            });
        }
        if matches!(self.peek().kind, TokenKind::As) {
            let as_span = self.peek().span;
            self.advance(); // consume 'as'
            let target = match self.parse_type() {
                Some(t) => t,
                None => return expr,
            };
            let target_end = self.tokens[self.current - 1].span;
            let span = expr.span().to(target_end);
            let id = self.node_id_gen.next();
            let cast = Expr::Cast(CastExpr {
                id,
                span,
                expr: Box::new(expr),
                target,
            });
            // Reject chained casts: `x as A as B`.
            if matches!(self.peek().kind, TokenKind::As) {
                let chain_span = as_span.to(self.peek().span);
                self.errors
                    .push(ParseError::ChainedCast { span: chain_span });
                // Consume the chained `as <Type>` for recovery so that one
                // chain doesn't yield a cascade of follow-on errors.
                self.advance(); // 'as'
                let _ = self.parse_type();
            }
            return cast;
        }
        expr
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
            // Prefix `await expr`. Same precedence tier as `-`/`!` — binds
            // tighter than binary operators, looser than postfix. Use parens
            // to chain into a method call: `(await fetch(url: u)).body`.
            //
            // **M9 desugaring (parse-time):** `await expr` is rewritten to
            // `perform Async::Suspend(future: expr)` here. No `Expr::Await`
            // node ever leaves the parser.
            //
            // Parser fast-path: top-level `await` (file scope, depth 0) is
            // rejected immediately. Inside any function body the type
            // checker takes over.
            TokenKind::Await => {
                let start_span = self.peek().span;
                self.advance();
                let operand = self.parse_unary_expr();
                let span = start_span.to(operand.span());
                if self.fn_depth == 0 {
                    self.errors.push(ParseError::AwaitOutsideAsync { span });
                }
                let id = self.node_id_gen.next();
                if let Some(syms) = self.desugar {
                    Expr::Perform(PerformExpr {
                        id,
                        span,
                        effect: syms.async_eff,
                        op: syms.suspend_op,
                        args: vec![(syms.future_param, operand)],
                    })
                } else {
                    // No interner — emit a unit literal placeholder so
                    // unit tests that don't provide an interner don't
                    // panic. The legacy `Expr::Await` shape was removed
                    // in M9 Task 7.
                    let _ = operand; // suppress unused warning
                    Expr::Literal {
                        value: Literal::Unit,
                        id,
                        span,
                    }
                }
            }
            // Prefix `yield expr`. Desugars at parse time to
            // `perform Gen::Yield(value: expr)`. Same fast-path rule as
            // `await`: a `yield` outside any fn body is a parse error.
            TokenKind::Yield => {
                let start_span = self.peek().span;
                self.advance();
                let value = self.parse_unary_expr();
                let span = start_span.to(value.span());
                if self.fn_depth == 0 {
                    self.errors.push(ParseError::AwaitOutsideAsync { span });
                }
                let id = self.node_id_gen.next();
                if let Some(syms) = self.desugar {
                    Expr::Perform(PerformExpr {
                        id,
                        span,
                        effect: syms.gen_eff,
                        op: syms.yield_op,
                        args: vec![(syms.value_param, value)],
                    })
                } else {
                    // No interner — emit a unit literal as a placeholder so
                    // unit tests that don't provide an interner don't panic.
                    Expr::Literal {
                        value: Literal::Unit,
                        id,
                        span,
                    }
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

    /// Returns true if the current position looks like an implicit-named-arg
    /// bare identifier (M10 Task 6 Part A). The token must be a plain ident
    /// followed by either `,` or `)` — anything else (`.`, `(`, operators,
    /// etc.) means it's a more complex expression that must use an explicit
    /// `name:` label.
    fn is_implicit_named_arg_start(&self) -> bool {
        if !matches!(self.peek().kind, TokenKind::Ident(_)) {
            return false;
        }
        let next = match self.tokens.get(self.current + 1) {
            Some(t) => &t.kind,
            None => return false,
        };
        matches!(next, TokenKind::Comma | TokenKind::RParen)
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
                                self.advance(); // consume name
                                self.advance(); // consume ':'
                                sym
                            } else {
                                unreachable!()
                            };
                            let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                            let span = arg_start.to(value.span());
                            args.push(NamedArg {
                                span,
                                name,
                                value: Box::new(value),
                                implicit: false,
                            });
                        } else if self.is_implicit_named_arg_start() {
                            // M10 Task 6 Part A: bare identifier — implicit named arg.
                            // `foo` at a call site (not followed by `:`) becomes
                            // `NamedArg { name: foo, value: Variable(foo), implicit: true }`.
                            let (name, ident_span) = match self.peek().kind {
                                TokenKind::Ident(sym) => (sym, self.peek().span),
                                _ => unreachable!(),
                            };
                            self.advance(); // consume the identifier
                            let id = self.node_id_gen.next();
                            let value = Expr::Variable {
                                name,
                                id,
                                span: ident_span,
                            };
                            args.push(NamedArg {
                                span: ident_span,
                                name,
                                value: Box::new(value),
                                implicit: true,
                            });
                        } else {
                            // Positional arg — error and recover
                            self.errors
                                .push(ParseError::PositionalArg { span: arg_start });
                            let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                            let span = arg_start.to(value.span());
                            // Use a sentinel name (sym 0) for error recovery
                            args.push(NamedArg {
                                span,
                                name: Symbol::new(0),
                                value: Box::new(value),
                                implicit: false,
                            });
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
            } else if self.check(&TokenKind::LBracket) {
                // Postfix indexing: `expr[index]`.
                self.advance(); // '['
                let index = self.with_struct_literal_allowed(|p| p.parse_expr());
                let end_span = if let Ok(tok) = self.expect(TokenKind::RBracket, "']'") {
                    tok.span
                } else {
                    self.peek().span
                };
                let span = expr.span().to(end_span);
                let id = self.node_id_gen.next();
                expr = Expr::Index {
                    array: Box::new(expr),
                    index: Box::new(index),
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
            } else if self.check(&TokenKind::Question) {
                // Postfix `?` propagation. Binds tighter than any binary
                // operator (M10 Task 4). Operand validity (Option<T> /
                // Result<T,E>) is checked in the type checker.
                let q_span = self.peek().span;
                self.advance();
                let span = expr.span().to(q_span);
                expr = Expr::Propagate(PropagateExpr {
                    span,
                    operand: Box::new(expr),
                });
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
                    self.advance();
                    self.advance();
                    sym
                } else {
                    unreachable!()
                };
                let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                let span = arg_start.to(value.span());
                args.push(NamedArg {
                    span,
                    name,
                    value: Box::new(value),
                    implicit: false,
                });
            } else if self.is_implicit_named_arg_start() {
                // M10 Task 6 Part A: bare identifier as implicit named arg.
                let (name, ident_span) = match self.peek().kind {
                    TokenKind::Ident(sym) => (sym, self.peek().span),
                    _ => unreachable!(),
                };
                self.advance();
                let id = self.node_id_gen.next();
                let value = Expr::Variable {
                    name,
                    id,
                    span: ident_span,
                };
                args.push(NamedArg {
                    span: ident_span,
                    name,
                    value: Box::new(value),
                    implicit: true,
                });
            } else {
                self.errors
                    .push(ParseError::PositionalArg { span: arg_start });
                let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                let span = arg_start.to(value.span());
                args.push(NamedArg {
                    span,
                    name: Symbol::new(0),
                    value: Box::new(value),
                    implicit: false,
                });
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
            // `async { ... }` block expression. The next token must be `{`;
            // anything else is `AsyncOnNonFn`.
            //
            // **M9 desugaring (parse-time):** `async { body }` desugars to
            // the block itself — its `await`s have already been rewritten
            // to `perform Async::Suspend(...)` by the prefix-`await` parser
            // arm. The implicit `Eff<[Async], T>` wrapping is recovered by
            // the type checker via row inference (Task 3); the parser does
            // NOT emit a closure-IIFE here because closures don't yet carry
            // explicit effect annotations. This is a documented deviation
            // from the m9-02 spec and is acceptable for M8 fixtures.
            TokenKind::Async => {
                let start_span = token.span;
                self.advance(); // consume 'async'
                if !self.check(&TokenKind::LBrace) {
                    let bad = self.peek().clone();
                    let span = start_span.to(bad.span);
                    self.errors.push(ParseError::AsyncOnNonFn { span });
                    return Expr::Literal {
                        value: Literal::Unit,
                        id,
                        span,
                    };
                }
                // Bump fn_depth so `await` inside the block is accepted by
                // the parse-time fast-path. Type checker still verifies the
                // operand type.
                self.fn_depth += 1;
                let block = self.parse_block();
                self.fn_depth -= 1;
                // Keep `Expr::AsyncBlock` even after parser desugaring: the
                // type checker uses it as a marker to bump `async_depth`,
                // which gates the M8 `AwaitOutsideAsync` diagnostic on a
                // sync-context `await`. The compiler simply unwraps it
                // back into its inner block — `Op::MakeAsync` is no longer
                // emitted around it because the M9 default `Async` handler
                // resolves the `perform Async::Suspend` sites directly.
                let span = start_span.to(block.span());
                Expr::AsyncBlock(AsyncBlockExpr {
                    id,
                    span,
                    block: Box::new(block),
                })
            }
            // Zero-parameter closure: `|| body` (body may be a block or expr).
            TokenKind::OrOr => {
                let start_span = token.span;
                self.advance(); // consume '||'
                self.fn_depth += 1;
                let body = self.parse_expr();
                self.fn_depth -= 1;
                let span = start_span.to(body.span());
                Expr::Closure {
                    params: vec![],
                    body: Box::new(body),
                    id,
                    span,
                }
            }
            // Closure with parameters: `|x, y| body` or `|x: Int| body`.
            TokenKind::Pipe => {
                // Reject stray `|` up front (e.g. `1 | 2`). A real closure
                // starts with `|` followed by `|` (empty params) or an
                // identifier (named param). Anything else triggers a single
                // clear `StrayPipe` error instead of cascading through the
                // closure-parameter parser.
                let next_kind = self.tokens.get(self.current + 1).map(|t| &t.kind);
                let looks_like_closure =
                    matches!(next_kind, Some(TokenKind::Pipe) | Some(TokenKind::Ident(_)));
                if !looks_like_closure {
                    let pipe_span = token.span;
                    self.errors.push(ParseError::StrayPipe { span: pipe_span });
                    self.advance(); // consume the stray '|'
                    return Expr::Literal {
                        value: Literal::Unit,
                        id,
                        span: pipe_span,
                    };
                }
                let start_span = token.span;
                self.advance(); // consume opening '|'

                let mut params: Vec<Param> = Vec::new();
                if !self.check(&TokenKind::Pipe) {
                    loop {
                        let param_start = self.peek().span;
                        let pname = match self.peek().kind {
                            TokenKind::Ident(s) => {
                                self.advance();
                                s
                            }
                            _ => {
                                let tok = self.peek().clone();
                                self.errors.push(ParseError::UnexpectedToken {
                                    expected: "closure parameter name".to_string(),
                                    found: tok.kind,
                                    span: tok.span,
                                });
                                Symbol::new(0)
                            }
                        };
                        let ty = if self.match_token(&TokenKind::Colon) {
                            self.parse_type().unwrap_or(TypeAnnotation::Infer)
                        } else {
                            TypeAnnotation::Infer
                        };
                        params.push(Param {
                            span: param_start.to(self.tokens[self.current.saturating_sub(1)].span),
                            name: pname,
                            ty,
                            default: None,
                        });
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let _ = self.expect(TokenKind::Pipe, "'|'");

                self.fn_depth += 1;
                let body = self.parse_expr();
                self.fn_depth -= 1;
                let span = start_span.to(body.span());
                Expr::Closure {
                    params,
                    body: Box::new(body),
                    id,
                    span,
                }
            }
            // Array literal: `[]`, `[1, 2, 3]`.
            TokenKind::LBracket => {
                let start_span = token.span;
                self.advance(); // '['
                let mut elements: Vec<Expr> = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        let elem = self.with_struct_literal_allowed(|p| p.parse_expr());
                        elements.push(elem);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end_span = if let Ok(tok) = self.expect(TokenKind::RBracket, "']'") {
                    tok.span
                } else {
                    self.peek().span
                };
                Expr::ArrayLit {
                    elements,
                    id,
                    span: start_span.to(end_span),
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
            // `f"..."` interpolated string. M10 (Task 2). The lexer has
            // already split the literal into FStringStart / alternating
            // FStringLit + FStringExprStart..FStringExprEnd / FStringEnd.
            TokenKind::FStringStart => self.parse_fstring_expr(id, token.span),
            // `perform Effect::Op(name: value, ...)`
            TokenKind::Perform => self.parse_perform_expr(id),
            // `handle { body } with { Effect::Op(args) => clause, ... }`
            TokenKind::Handle => self.parse_handle_expr(id),
            // `resume k with v`
            TokenKind::Resume => self.parse_resume_expr(id),
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
                    self.errors
                        .push(ParseError::InvalidRequireMode { span: tok.span });
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
                if self.match_token(&TokenKind::Comma) && self.is_set_label_start() {
                    set_fn = self.parse_set_clause();
                }
            }
        }

        // Optional trailing semicolon
        self.match_token(&TokenKind::Semi);

        let end_span = set_fn
            .as_ref()
            .map(|e| e.span())
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
    /// Parses an `f"..."` interpolated string. The caller has confirmed the
    /// next token is `FStringStart` and allocated `id`. M10 (Task 2).
    ///
    /// Token shape from the lexer:
    /// `FStringStart  ( FStringLit | FStringExprStart <expr> FStringExprEnd )*  FStringEnd`
    ///
    /// On premature EOF (no `FStringEnd`), emits `ParseError::UnterminatedFString`.
    fn parse_fstring_expr(&mut self, id: NodeId, start_span: Span) -> Expr {
        self.advance(); // consume FStringStart
        let mut segments: Vec<FStringSegment> = Vec::new();
        let end_span;

        loop {
            // Bail out gracefully on EOF — the f-string was unterminated.
            if matches!(self.peek().kind, TokenKind::Eof) {
                let span = start_span.to(self.peek().span);
                self.errors.push(ParseError::UnterminatedFString { span });
                return Expr::FString(FStringExpr {
                    id,
                    span,
                    segments,
                });
            }

            let tok = self.peek().clone();
            match tok.kind {
                TokenKind::FStringEnd => {
                    self.advance();
                    end_span = tok.span;
                    break;
                }
                TokenKind::FStringLit(s) => {
                    self.advance();
                    segments.push(FStringSegment {
                        span: tok.span,
                        kind: FStringSegmentKind::Lit(s),
                    });
                }
                TokenKind::FStringExprStart => {
                    let brace_span = tok.span;
                    self.advance(); // consume `{`
                    // The inner expression may itself be any Ferric expression,
                    // including another f-string (the lexer pushes a fresh
                    // FStringStart on the stack and we recurse via parse_expr).
                    let prev_errors = self.errors.len();
                    let expr = self.parse_expr();
                    let new_errors = self.errors.len() - prev_errors;
                    // Expect FStringExprEnd. If we don't see it (e.g. EOF or
                    // some structural failure inside the expression), surface
                    // InvalidFStringExpr.
                    if matches!(self.peek().kind, TokenKind::FStringExprEnd) {
                        let end_brace = self.peek().span;
                        self.advance();
                        if new_errors > 0 {
                            // Inner expression had errors. Treat as invalid.
                            self.errors.push(ParseError::InvalidFStringExpr {
                                span: brace_span.to(end_brace),
                            });
                        }
                        segments.push(FStringSegment {
                            span: brace_span.to(end_brace),
                            kind: FStringSegmentKind::Expr(Box::new(expr)),
                        });
                    } else {
                        // Could not close the interpolation cleanly.
                        let here = self.peek().span;
                        self.errors.push(ParseError::InvalidFStringExpr {
                            span: brace_span.to(here),
                        });
                        segments.push(FStringSegment {
                            span: brace_span.to(here),
                            kind: FStringSegmentKind::Expr(Box::new(expr)),
                        });
                        // Try to recover: if EOF, bail; otherwise loop and let
                        // the outer match handle whatever comes next.
                        if matches!(self.peek().kind, TokenKind::Eof) {
                            let span = start_span.to(self.peek().span);
                            self.errors.push(ParseError::UnterminatedFString { span });
                            return Expr::FString(FStringExpr {
                                id,
                                span,
                                segments,
                            });
                        }
                    }
                }
                _ => {
                    // Unexpected token inside f-string. Surface and bail.
                    let span = start_span.to(tok.span);
                    self.errors.push(ParseError::UnterminatedFString { span });
                    return Expr::FString(FStringExpr {
                        id,
                        span,
                        segments,
                    });
                }
            }
        }

        Expr::FString(FStringExpr {
            id,
            span: start_span.to(end_span),
            segments,
        })
    }

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
                    sub_parser.node_id_gen = NodeIdGen {
                        next: self.node_id_gen.next,
                    };
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

    /// Parses a `perform Effect::Op(name: value, ...)` expression. The
    /// caller has already determined that the next token is `Perform` and
    /// allocated a `NodeId`.
    fn parse_perform_expr(&mut self, id: NodeId) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'perform'

        // Effect name (identifier).
        let effect = match self.peek().kind {
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "effect name".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return Expr::Literal {
                    value: Literal::Unit,
                    id,
                    span: start_span,
                };
            }
        };

        // `::Op` separator + op name.
        if self.expect(TokenKind::ColonColon, "'::'").is_err() {
            return Expr::Literal {
                value: Literal::Unit,
                id,
                span: start_span,
            };
        }
        let op = match self.peek().kind {
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "operation name".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return Expr::Literal {
                    value: Literal::Unit,
                    id,
                    span: start_span,
                };
            }
        };

        // Argument list: `(name: value, ...)`. Like regular calls, args are
        // named; a positional arg is reported as `PositionalArg`.
        if self.expect(TokenKind::LParen, "'('").is_err() {
            return Expr::Literal {
                value: Literal::Unit,
                id,
                span: start_span,
            };
        }

        let mut args: Vec<(Symbol, Expr)> = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let arg_start = self.peek().span;
                if self.is_named_arg_start() {
                    let name = if let TokenKind::Ident(s) = self.peek().kind {
                        self.advance(); // name
                        self.advance(); // ':'
                        s
                    } else {
                        unreachable!()
                    };
                    let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                    args.push((name, value));
                } else {
                    self.errors
                        .push(ParseError::PositionalArg { span: arg_start });
                    let value = self.with_struct_literal_allowed(|p| p.parse_expr());
                    args.push((Symbol::new(0), value));
                }
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

        Expr::Perform(PerformExpr {
            id,
            span: start_span.to(end_span),
            effect,
            op,
            args,
        })
    }

    /// Parses a `handle { body } with { Effect::Op(arg, ...) => body, ... }`
    /// expression. The continuation variable `k` is implicitly bound inside
    /// each clause body; users may rename it by naming a parameter `k`
    /// (which produces a `ShadowsContinuation` warning).
    fn parse_handle_expr(&mut self, id: NodeId) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'handle'

        // The body must be a `{ ... }` block.
        let body = if self.check(&TokenKind::LBrace) {
            self.parse_block()
        } else {
            let tok = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "'{' to start the handle body".to_string(),
                found: tok.kind,
                span: tok.span,
            });
            Expr::Literal {
                value: Literal::Unit,
                id: self.node_id_gen.next(),
                span: start_span,
            }
        };

        // `with` keyword.
        if self.expect(TokenKind::With, "'with'").is_err() {
            return Expr::Literal {
                value: Literal::Unit,
                id,
                span: start_span,
            };
        }

        if self.expect(TokenKind::LBrace, "'{'").is_err() {
            return Expr::Literal {
                value: Literal::Unit,
                id,
                span: start_span,
            };
        }

        let cont_default = self
            .desugar
            .map(|d| d.cont_default)
            .unwrap_or(Symbol::new(0));

        let mut clauses: Vec<HandlerClause> = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let clause_start = self.peek().span;

            // Effect name.
            let effect = match self.peek().kind {
                TokenKind::Ident(s) => {
                    self.advance();
                    s
                }
                _ => {
                    let tok = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "effect name".to_string(),
                        found: tok.kind,
                        span: tok.span,
                    });
                    break;
                }
            };

            if self.expect(TokenKind::ColonColon, "'::'").is_err() {
                break;
            }

            let op = match self.peek().kind {
                TokenKind::Ident(s) => {
                    self.advance();
                    s
                }
                _ => {
                    let tok = self.peek().clone();
                    self.errors.push(ParseError::UnexpectedToken {
                        expected: "operation name".to_string(),
                        found: tok.kind,
                        span: tok.span,
                    });
                    break;
                }
            };

            // Parameter list: `(p1, p2, ...)` — bare names that bind the
            // operation's argument values.
            if self.expect(TokenKind::LParen, "'('").is_err() {
                break;
            }
            let mut params: Vec<Symbol> = Vec::new();
            if !self.check(&TokenKind::RParen) {
                loop {
                    let pspan = self.peek().span;
                    let pname = match self.peek().kind {
                        TokenKind::Ident(s) => {
                            self.advance();
                            s
                        }
                        _ => {
                            let tok = self.peek().clone();
                            self.errors.push(ParseError::UnexpectedToken {
                                expected: "parameter binding name".to_string(),
                                found: tok.kind,
                                span: tok.span,
                            });
                            break;
                        }
                    };
                    if pname == cont_default {
                        self.warnings
                            .push(ParseWarning::ShadowsContinuation { span: pspan });
                    }
                    params.push(pname);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            let _ = self.expect(TokenKind::RParen, "')'");

            // `=>` then handler body expression.
            if self.expect(TokenKind::FatArrow, "'=>'").is_err() {
                break;
            }

            // Track that we're inside a handler clause body — `resume k`
            // requires this scope.
            self.cont_stack.push(cont_default);
            let handler = self.parse_expr();
            self.cont_stack.pop();

            // Optional trailing comma between clauses.
            self.match_token(&TokenKind::Comma);

            clauses.push(HandlerClause {
                span: clause_start.to(handler.span()),
                effect,
                op,
                params,
                cont: cont_default,
                handler: Box::new(handler),
            });
        }

        let end_span = if let Ok(tok) = self.expect(TokenKind::RBrace, "'}'") {
            tok.span
        } else {
            self.peek().span
        };

        Expr::Handle(HandleExpr {
            id,
            span: start_span.to(end_span),
            body: Box::new(body),
            clauses,
        })
    }

    /// Parses a `resume k with value` expression. Validates that `resume`
    /// appears inside a handler clause body (`cont_stack` non-empty); emits
    /// `ParseError::ResumeOutsideHandler` otherwise.
    fn parse_resume_expr(&mut self, id: NodeId) -> Expr {
        let start_span = self.peek().span;
        self.advance(); // consume 'resume'

        let cont = match self.peek().kind {
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            _ => {
                let tok = self.peek().clone();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "continuation variable name".to_string(),
                    found: tok.kind,
                    span: tok.span,
                });
                return Expr::Literal {
                    value: Literal::Unit,
                    id,
                    span: start_span,
                };
            }
        };

        // `with` keyword.
        if !matches!(self.peek().kind, TokenKind::With) {
            let tok = self.peek().clone();
            self.errors.push(ParseError::UnexpectedToken {
                expected: "'with'".to_string(),
                found: tok.kind,
                span: tok.span,
            });
            return Expr::Literal {
                value: Literal::Unit,
                id,
                span: start_span,
            };
        }
        self.advance(); // consume 'with'

        let value = self.parse_expr();
        let span = start_span.to(value.span());

        // Validate that we're inside a handler clause.
        if self.cont_stack.is_empty() {
            self.errors.push(ParseError::ResumeOutsideHandler { span });
        }

        Expr::Resume(ResumeExpr {
            id,
            span,
            cont,
            value: Box::new(value),
        })
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

/// Pre-interns the well-known names the parser needs for `async`/`gen`/
/// effects desugaring (e.g. `Async`, `Suspend`, `Gen`, `Yield`, `value`,
/// `future`, `k`). Callers that use [`parse_with_interner`] should call
/// this once on the interner before parsing so that the desugared AST
/// references the correct `Symbol` identities.
pub fn pre_intern_desugar_names(interner: &mut Interner) {
    interner.intern("Async");
    interner.intern("Suspend");
    interner.intern("future");
    interner.intern("Gen");
    interner.intern("Yield");
    interner.intern("value");
    interner.intern("k");
    // `Unit` is the inner result type of `gen fn` after desugaring. The
    // resolver/typechecker recognise it by name, so interning it here ensures
    // the parser-emitted `TypeAnnotation::Named(Symbol("Unit"))` round-trips.
    interner.intern("Unit");
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
    ParseResult::with_warnings(items, parser.errors, parser.warnings)
}

/// Like [`parse`], but with access to the interner so that import-path strings
/// can be classified at parse time. M7+ programs that use `import` declarations
/// must call this entry point.
///
/// **Note:** the parser also uses the interner to look up well-known names
/// for `async`/`gen` desugaring. Callers that have a freshly constructed
/// interner should call [`pre_intern_desugar_names`] first to guarantee
/// those symbols are present.
pub fn parse_with_interner(lex: &LexResult, interner: &Interner) -> ParseResult {
    let mut parser = Parser::new_with_interner(&lex.tokens, interner);
    let items = parser.parse_program();
    ParseResult::with_warnings(items, parser.errors, parser.warnings)
}

/// Convenience wrapper that runs `parse` and serialises the result as JSON.
///
/// External tools that consume the AST should call `ferric --dump-ast` rather
/// than depending on this crate directly; this helper exists so that consumers
/// inside the workspace (e.g. the CLI) can produce the same output without
/// re-implementing the serialisation step.
pub fn parse_to_json(lex: &LexResult, interner: &Interner) -> Result<String, serde_json::Error> {
    let result = parse_with_interner(lex, interner);
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
            Item::Fn(FnItem {
                name, params, body, ..
            }) => {
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
            Item::Fn(FnItem {
                name,
                params,
                ret_ty,
                ..
            }) => {
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
            Item::Fn(FnItem { body, .. }) => {
                match body {
                    Expr::Block {
                        expr: Some(expr), ..
                    } => {
                        // Should be: 1 + (2 * 3)
                        match expr.as_ref() {
                            Expr::Binary {
                                op: BinOp::Add,
                                left,
                                right,
                                ..
                            } => {
                                // Left should be literal 1
                                match left.as_ref() {
                                    Expr::Literal {
                                        value: Literal::Int(1),
                                        ..
                                    } => {}
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
            Item::Fn(FnItem { body, .. }) => match body {
                Expr::Block {
                    expr: Some(expr), ..
                } => match expr.as_ref() {
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
        assert!(matches!(
            result.errors[0],
            ferric_common::ParseError::PositionalArg { .. }
        ));
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
        assert!(!result.errors.is_empty());
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

    // =============== M7 — Imports / Exports / Type aliases / Casts ===============

    /// Lex+parse helper that wires up an interner so import paths classify.
    fn lex_parse(src: &str) -> (Interner, ParseResult) {
        let mut interner = Interner::new();
        let lex_result = ferric_lexer::lex(src, &mut interner);
        pre_intern_desugar_names(&mut interner);
        let parsed = parse_with_interner(&lex_result, &interner);
        (interner, parsed)
    }

    #[test]
    fn test_named_import_relative() {
        let (interner, result) = lex_parse(r#"import { connect, disconnect as d } from "./db""#);
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        assert_eq!(result.items.len(), 1);
        match &result.items[0] {
            Item::Import(decl) => {
                assert!(matches!(decl.path, ImportPath::Relative(ref s) if s == "./db"));
                match &decl.items {
                    ImportItems::Named(items) => {
                        assert_eq!(items.len(), 2);
                        assert_eq!(interner.resolve(items[0].name), "connect");
                        assert!(items[0].alias.is_none());
                        assert_eq!(interner.resolve(items[1].name), "disconnect");
                        assert_eq!(
                            items[1].alias.map(|s| interner.resolve(s).to_string()),
                            Some("d".to_string())
                        );
                    }
                    other => panic!("expected named imports, got {other:?}"),
                }
            }
            other => panic!("expected import, got {other:?}"),
        }
    }

    #[test]
    fn test_namespace_import() {
        let (interner, result) = lex_parse(r#"import * as db from "./db""#);
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Import(decl) => match &decl.items {
                ImportItems::Namespace(sym) => assert_eq!(interner.resolve(*sym), "db"),
                other => panic!("expected namespace import, got {other:?}"),
            },
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn test_workspace_import_path() {
        let (_, result) = lex_parse(r#"import { Config } from "@/config""#);
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Import(decl) => {
                assert!(matches!(decl.path, ImportPath::Workspace(ref s) if s == "@/config"))
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn test_cache_import_path() {
        let (_, result) = lex_parse(r#"import { HttpClient } from "ferric-http""#);
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Import(decl) => {
                assert!(matches!(decl.path, ImportPath::Cache(ref s) if s == "ferric-http"))
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn test_invalid_import_path() {
        let (_, result) = lex_parse(r#"import { X } from "not/a/valid/cache/name""#);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::InvalidImportPath { .. }))
        );
    }

    #[test]
    fn test_default_import_is_error() {
        let (_, result) = lex_parse(r#"import db from "./db""#);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::DefaultImport { .. }))
        );
    }

    #[test]
    fn test_late_import_is_error() {
        let src = "fn foo() { }\nimport { bar } from \"./bar\"\n";
        let (_, result) = lex_parse(src);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::LateImport { .. }))
        );
        // Only the legal item (the fn) should remain.
        assert_eq!(result.items.len(), 1);
        assert!(matches!(result.items[0], Item::Fn(_)));
    }

    #[test]
    fn test_export_fn() {
        let (_, result) = lex_parse("export fn connect() { }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Export(decl) => assert!(matches!(*decl.item, Item::Fn(_))),
            _ => panic!("expected export"),
        }
    }

    #[test]
    fn test_export_struct() {
        let (_, result) = lex_parse("export struct Config { host: Str }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Export(decl) => assert!(matches!(*decl.item, Item::StructDef { .. })),
            _ => panic!("expected export"),
        }
    }

    #[test]
    fn test_export_enum() {
        let (_, result) = lex_parse("export enum Color { Red, Green }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Export(decl) => assert!(matches!(*decl.item, Item::EnumDef { .. })),
            _ => panic!("expected export"),
        }
    }

    #[test]
    fn test_export_type_alias() {
        let (_, result) = lex_parse("export type Url = Str");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Export(decl) => assert!(matches!(*decl.item, Item::TypeAlias(_))),
            _ => panic!("expected export"),
        }
    }

    #[test]
    fn test_export_inside_fn_body_is_error() {
        let src = "fn foo() { export let x = 1 }";
        let (_, result) = lex_parse(src);
        // The body parser doesn't recognise `export`, so it should fall through
        // to an UnexpectedToken / ExpectedExpression for the keyword. Either
        // way an error is recorded — we just confirm at least one parse error.
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_type_alias_simple() {
        let (interner, result) = lex_parse("type Url = Str");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::TypeAlias(decl) => {
                assert_eq!(interner.resolve(decl.name), "Url");
                assert_eq!(decl.params.len(), 0);
                assert!(decl.opaque);
                assert!(
                    matches!(&decl.ty, TypeAnnotation::Named(s) if interner.resolve(*s) == "Str")
                );
            }
            _ => panic!("expected type alias"),
        }
    }

    #[test]
    fn test_type_alias_generic() {
        let (interner, result) = lex_parse("type Pair<A, B> = Tuple<A, B>");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::TypeAlias(decl) => {
                assert_eq!(interner.resolve(decl.name), "Pair");
                assert_eq!(decl.params.len(), 2);
            }
            _ => panic!("expected type alias"),
        }
    }

    #[test]
    fn test_cast_expression() {
        // `let u = "x" as Url`
        let (_, result) = lex_parse(r#"let u = "x" as Url"#);
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Script {
                stmt: Stmt::Let { init, .. },
                ..
            } => {
                assert!(matches!(init, Expr::Cast(_)));
            }
            _ => panic!("expected let"),
        }
    }

    #[test]
    fn test_cast_precedence_with_binary() {
        // `let r = a + b as Int` should parse as `a + (b as Int)`.
        let (_, result) = lex_parse("let r = a + b as Int");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Script {
                stmt: Stmt::Let { init, .. },
                ..
            } => match init {
                Expr::Binary {
                    op: BinOp::Add,
                    right,
                    ..
                } => {
                    assert!(matches!(**right, Expr::Cast(_)));
                }
                other => panic!("expected `+` at the root, got {other:?}"),
            },
            _ => panic!("expected let"),
        }
    }

    #[test]
    fn test_cast_precedence_with_field_access() {
        // `let u = obj.field as Url` should parse as `(obj.field) as Url`.
        let (_, result) = lex_parse("let u = obj.field as Url");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Script {
                stmt: Stmt::Let { init, .. },
                ..
            } => match init {
                Expr::Cast(c) => assert!(matches!(*c.expr, Expr::FieldAccess { .. })),
                other => panic!("expected cast at the root, got {other:?}"),
            },
            _ => panic!("expected let"),
        }
    }

    #[test]
    fn test_chained_cast_is_error() {
        let (_, result) = lex_parse("let x = y as Foo as Bar");
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::ChainedCast { .. }))
        );
    }

    // =============== M8 — async fn / await / async blocks ===============

    #[test]
    fn test_async_fn_top_level() {
        // M9: `async fn` desugars to a plain `Item::Fn` with return type
        // `Eff<[Async], T>`. No `Item::AsyncFn` ever appears in the AST.
        let (interner, result) = lex_parse("async fn fetch(url: Str) -> Str { url }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Fn(item) => {
                assert_eq!(interner.resolve(item.name), "fetch");
                assert_eq!(item.params.len(), 1);
                match &item.ret_ty {
                    TypeAnnotation::Eff { row, result: inner } => {
                        assert_eq!(row.len(), 1);
                        assert_eq!(interner.resolve(row[0].name), "Async");
                        assert!(matches!(inner.as_ref(), TypeAnnotation::Named(_)));
                    }
                    other => panic!("expected Eff return type, got {other:?}"),
                }
            }
            other => panic!("expected Item::Fn after async desugaring, got {other:?}"),
        }
    }

    #[test]
    fn test_async_on_non_fn_is_error() {
        let (_, result) = lex_parse("async let x = 5");
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::AsyncOnNonFn { .. })),
            "errors: {:?}",
            result.errors,
        );
    }

    #[test]
    fn test_async_struct_is_error() {
        let (_, result) = lex_parse("async struct Foo { }");
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::AsyncOnNonFn { .. })),
            "errors: {:?}",
            result.errors,
        );
    }

    #[test]
    fn test_await_prefix_inside_async_fn() {
        // M9: `await expr` desugars to `perform Async::Suspend(future: expr)`.
        // The wrapping `async fn` desugars to `Item::Fn` with `Eff<[Async], T>`.
        let (interner, result) = lex_parse("async fn f() -> Int { await foo() }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Fn(item) => match &item.body {
                Expr::Block {
                    expr: Some(tail), ..
                } => match tail.as_ref() {
                    Expr::Perform(p) => {
                        assert_eq!(interner.resolve(p.effect), "Async");
                        assert_eq!(interner.resolve(p.op), "Suspend");
                        assert_eq!(p.args.len(), 1);
                        assert_eq!(interner.resolve(p.args[0].0), "future");
                    }
                    other => panic!("tail expr should be Perform, got {other:?}"),
                },
                other => panic!("expected block body, got {other:?}"),
            },
            other => panic!("expected Item::Fn after desugaring, got {other:?}"),
        }
    }

    #[test]
    fn test_await_with_field_access_needs_parens() {
        // `await foo().x` parses as `await (foo().x)` — the postfix `.x`
        // binds tighter than the prefix await.
        let (_, result) = lex_parse("async fn f() -> Int { await obj().x }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
    }

    #[test]
    fn test_await_then_method_call_needs_parens() {
        // To call `.len()` on the awaited value, parens are required.
        let (_, result) = lex_parse("async fn f() -> Int { (await fetch()).len() }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
    }

    #[test]
    fn test_double_await_chain() {
        // `await await nested()` — parser must accept; type checker rejects
        // later if the inner await isn't well-typed.
        let (_, result) = lex_parse("async fn f() -> Int { await await nested() }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
    }

    #[test]
    fn test_await_at_file_top_level_is_error() {
        // No enclosing function — fast-path parse error fires.
        let (_, result) = lex_parse("let x = await foo()");
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::AwaitOutsideAsync { .. })),
            "errors: {:?}",
            result.errors,
        );
    }

    #[test]
    fn test_await_inside_sync_fn_is_not_parse_error() {
        // Parser defers to the type checker for awaits inside any function
        // body — sync or async — because the parser cannot know macro/future
        // expansion outcomes.
        let (_, result) = lex_parse("fn f() -> Int { await bar() }");
        assert!(
            !result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::AwaitOutsideAsync { .. })),
            "parser should defer to typechecker; got: {:?}",
            result.errors,
        );
    }

    #[test]
    fn test_async_block_expression() {
        // M9 Task 6: `async { ... }` survives the parser as
        // `Expr::AsyncBlock(...)`. The marker is what the type checker
        // reads to bump `async_depth` (so awaits inside accept the
        // parser's `perform Async::Suspend` desugaring) and what the
        // compiler reads to inline-emit the body — there is no longer a
        // `MakeAsync` deferral around it. The inner `await`s have
        // already been rewritten to `perform`.
        let (_, result) = lex_parse("let task = async { 42 }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Script {
                stmt: Stmt::Let { init, .. },
                ..
            } => {
                assert!(
                    matches!(init, Expr::AsyncBlock(_)),
                    "init should be `Expr::AsyncBlock`, got {init:?}"
                );
            }
            other => panic!("expected let, got {other:?}"),
        }
    }

    #[test]
    fn test_empty_async_block() {
        let (_, result) = lex_parse("let t = async { }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
    }

    #[test]
    fn test_await_inside_async_block_is_legal() {
        // `await` is fine inside an async{} block at file top level — the
        // block bumps fn_depth.
        let (_, result) = lex_parse("let r = async { await foo() }");
        assert!(
            !result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::AwaitOutsideAsync { .. })),
            "errors: {:?}",
            result.errors,
        );
    }

    #[test]
    fn test_async_type_annotation_parses_as_generic() {
        // `Async<Int>` parses as TypeAnnotation::Generic { head: "Async", args: [Int] }.
        let (interner, result) = lex_parse("let t: Async<Int> = async { 1 }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Script {
                stmt: Stmt::Let { ty: Some(ty), .. },
                ..
            } => match ty {
                TypeAnnotation::Generic { head, args } => {
                    assert_eq!(interner.resolve(*head), "Async");
                    assert_eq!(args.len(), 1);
                }
                other => panic!("expected Generic type annotation, got {other:?}"),
            },
            other => panic!("expected let with type annotation, got {other:?}"),
        }
    }

    #[test]
    fn test_handle_type_annotation_parses_as_generic() {
        let (interner, result) = lex_parse("fn run(h: Handle<Str>) -> Unit { }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Fn(item) => match &item.params[0].ty {
                TypeAnnotation::Generic { head, .. } => {
                    assert_eq!(interner.resolve(*head), "Handle");
                }
                other => panic!("expected Generic, got {other:?}"),
            },
            other => panic!("expected fn, got {other:?}"),
        }
    }

    // =============== M9 — effects: declarations, perform, handle, resume ===============

    #[test]
    fn test_effect_decl_parses() {
        let (interner, result) =
            lex_parse("effect Counter { Inc(amount: Int) -> Int, Reset() -> Unit }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::EffectDecl(decl) => {
                assert_eq!(interner.resolve(decl.name), "Counter");
                assert_eq!(decl.ops.len(), 2);
                assert_eq!(interner.resolve(decl.ops[0].name), "Inc");
                assert_eq!(decl.ops[0].params.len(), 1);
                assert_eq!(interner.resolve(decl.ops[0].params[0].0), "amount");
                assert_eq!(decl.ops[0].resume_type, Ty::Int);
                assert_eq!(interner.resolve(decl.ops[1].name), "Reset");
                assert_eq!(decl.ops[1].params.len(), 0);
                assert_eq!(decl.ops[1].resume_type, Ty::Unit);
            }
            other => panic!("expected EffectDecl, got {other:?}"),
        }
    }

    #[test]
    fn test_effect_decl_with_type_params() {
        let (interner, result) = lex_parse("effect Gen<T> { Yield(value: T) -> Unit }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::EffectDecl(decl) => {
                assert_eq!(interner.resolve(decl.name), "Gen");
                assert_eq!(decl.type_params.len(), 1);
                assert_eq!(interner.resolve(decl.type_params[0]), "T");
            }
            other => panic!("expected EffectDecl, got {other:?}"),
        }
    }

    #[test]
    fn test_effect_decl_inside_fn_is_error() {
        let (_, result) = lex_parse("fn outer() -> Unit { effect Inner { Op() -> Unit } }");
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::EffectDeclInsideFn { .. })),
            "errors: {:?}",
            result.errors,
        );
    }

    #[test]
    fn test_perform_expression() {
        let (interner, result) =
            lex_parse("fn use_eff() -> Int { perform Counter::Inc(amount: 1) }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Fn(item) => match &item.body {
                Expr::Block {
                    expr: Some(tail), ..
                } => match tail.as_ref() {
                    Expr::Perform(p) => {
                        assert_eq!(interner.resolve(p.effect), "Counter");
                        assert_eq!(interner.resolve(p.op), "Inc");
                        assert_eq!(p.args.len(), 1);
                        assert_eq!(interner.resolve(p.args[0].0), "amount");
                    }
                    other => panic!("expected Perform tail, got {other:?}"),
                },
                other => panic!("expected block body, got {other:?}"),
            },
            other => panic!("expected fn, got {other:?}"),
        }
    }

    #[test]
    fn test_handle_with_clauses() {
        let (interner, result) = lex_parse(
            "fn run() -> Int { handle { 1 } with { Counter::Inc(n) => resume k with n } }",
        );
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Fn(item) => match &item.body {
                Expr::Block {
                    expr: Some(tail), ..
                } => match tail.as_ref() {
                    Expr::Handle(h) => {
                        assert_eq!(h.clauses.len(), 1);
                        let cl = &h.clauses[0];
                        assert_eq!(interner.resolve(cl.effect), "Counter");
                        assert_eq!(interner.resolve(cl.op), "Inc");
                        assert_eq!(cl.params.len(), 1);
                        assert_eq!(interner.resolve(cl.cont), "k");
                        // Inside handler clause, `resume k with n` is legal
                        // and parses as Expr::Resume.
                        match cl.handler.as_ref() {
                            Expr::Resume(r) => {
                                assert_eq!(interner.resolve(r.cont), "k");
                            }
                            other => panic!("expected Resume, got {other:?}"),
                        }
                    }
                    other => panic!("expected Handle tail, got {other:?}"),
                },
                other => panic!("expected block body, got {other:?}"),
            },
            other => panic!("expected fn, got {other:?}"),
        }
    }

    #[test]
    fn test_resume_outside_handler_is_error() {
        let (_, result) = lex_parse("fn bad() -> Int { resume k with 1 }");
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ParseError::ResumeOutsideHandler { .. })),
            "errors: {:?}",
            result.errors,
        );
    }

    #[test]
    fn test_handler_param_named_k_warns_shadows_continuation() {
        let (_, result) = lex_parse("fn run() -> Int { handle { 1 } with { Foo::Bar(k) => 0 } }");
        assert!(
            result
                .warnings
                .iter()
                .any(|w| matches!(w, ParseWarning::ShadowsContinuation { .. })),
            "warnings: {:?}",
            result.warnings,
        );
    }

    #[test]
    fn test_gen_fn_desugars_to_eff_gen_unit() {
        let (interner, result) = lex_parse("gen fn iter() -> Int { yield 42 }");
        assert_eq!(result.errors.len(), 0, "errors: {:?}", result.errors);
        match &result.items[0] {
            Item::Fn(item) => {
                match &item.ret_ty {
                    TypeAnnotation::Eff { row, result: inner } => {
                        assert_eq!(row.len(), 1);
                        assert_eq!(interner.resolve(row[0].name), "Gen");
                        assert_eq!(row[0].args.len(), 1);
                        // Inner result should be Unit.
                        match inner.as_ref() {
                            TypeAnnotation::Named(s) => {
                                assert_eq!(interner.resolve(*s), "Unit");
                            }
                            other => panic!("expected Unit return, got {other:?}"),
                        }
                    }
                    other => panic!("expected Eff return, got {other:?}"),
                }
                // Body's tail expr should be Perform(Gen::Yield).
                match &item.body {
                    Expr::Block {
                        expr: Some(tail), ..
                    } => match tail.as_ref() {
                        Expr::Perform(p) => {
                            assert_eq!(interner.resolve(p.effect), "Gen");
                            assert_eq!(interner.resolve(p.op), "Yield");
                            assert_eq!(interner.resolve(p.args[0].0), "value");
                        }
                        other => panic!("expected Perform tail, got {other:?}"),
                    },
                    other => panic!("expected block body, got {other:?}"),
                }
            }
            other => panic!("expected Item::Fn, got {other:?}"),
        }
    }

    // The legacy `Item::AsyncFn` / `Expr::Await` survival test that lived
    // here was removed in M9 Task 7 along with the variants themselves —
    // the parser cannot construct shapes that no longer exist in the AST.
}
