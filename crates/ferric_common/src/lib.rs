//! # Ferric Common
//!
//! Shared types and utilities used across all Ferric pipeline stages.
//!
//! This crate is the foundation of the Ferric architecture and is depended upon
//! by all other stages, but never depends on them. This enables surgical stage
//! replacement without cascading changes.

// Re-export all modules
pub use span::*;
pub use identifiers::*;
pub use interner::*;
pub use tokens::*;
pub use types::*;
pub use errors::*;
pub use results::*;
pub use ast::*;

mod span;
mod identifiers;
mod interner;
mod tokens;
mod types;
mod errors;
mod results;
mod ast;

/// Serialises a `ParseResult` as pretty-printed JSON.
///
/// External tools consume Ferric's AST through this entry point — they import
/// `ferric_common` only and never depend on stage internals.
pub fn ast_to_json(ast: &ParseResult) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(ast)
}

/// Deserialises a `ParseResult` from JSON produced by `ast_to_json`.
pub fn ast_from_json(s: &str) -> Result<ParseResult, serde_json::Error> {
    serde_json::from_str(s)
}

// Compile-time assertion: every public type that crosses the pipeline must be
// `Send + Sync` so a future async runtime can carry it across `.await` points.
// If a new type fails this check, fix the offending field (no `Rc`/`RefCell`/
// raw pointers) — do not weaken the bound. See `ferric_vm/ASYNC_COMPAT.md`.
const _: fn() = || {
    fn check<T: Send + Sync>() {}
    check::<Span>();
    check::<NodeId>();
    check::<Symbol>();
    check::<DefId>();
    check::<Interner>();
    check::<Token>();
    check::<TokenKind>();
    check::<ShellTokenPart>();
    check::<Ty>();
    check::<TypeAnnotation>();
    check::<Literal>();
    check::<BinOp>();
    check::<UnOp>();
    check::<Item>();
    check::<Expr>();
    check::<Stmt>();
    check::<Param>();
    check::<NamedArg>();
    check::<RequireStmt>();
    check::<RequireMode>();
    check::<ShellPart>();
    check::<ShellOutput>();
    check::<LexError>();
    check::<ParseError>();
    check::<ResolveError>();
    check::<TypeError>();
    check::<LexResult>();
    check::<ParseResult>();
    check::<ResolveResult>();
    check::<TypeResult>();
    check::<Program>();
    check::<Chunk>();
};

#[cfg(test)]
mod ast_serde_tests {
    use super::*;
    use std::collections::HashMap;

    fn round_trip(result: ParseResult) {
        let json = ast_to_json(&result).expect("serialise");
        let parsed: ParseResult = ast_from_json(&json).expect("deserialise");
        assert_eq!(parsed, result);
    }

    #[test]
    fn empty_parse_result_round_trips() {
        round_trip(ParseResult::new(vec![], vec![]));
    }

    #[test]
    fn ast_with_items_round_trips() {
        // A small but representative program: `let x: Int = 1 + 2;`
        let span = Span::new(0, 12);
        let id = NodeId::new(0);
        let int_sym = Symbol::new(7);
        let x_sym = Symbol::new(8);

        let init = Expr::Binary {
            op: BinOp::Add,
            left: Box::new(Expr::Literal {
                value: Literal::Int(1),
                id: NodeId::new(1),
                span,
            }),
            right: Box::new(Expr::Literal {
                value: Literal::Int(2),
                id: NodeId::new(2),
                span,
            }),
            id: NodeId::new(3),
            span,
        };

        let items = vec![Item::Script {
            stmt: Stmt::Let {
                name: x_sym,
                mutable: false,
                ty: Some(TypeAnnotation::Named(int_sym)),
                init,
                id: NodeId::new(4),
                span,
            },
            id,
            span,
        }];

        round_trip(ParseResult::new(items, vec![]));
    }

    #[test]
    fn parse_errors_round_trip() {
        let span = Span::new(0, 1);
        let errors = vec![ParseError::PositionalArg { span }];
        round_trip(ParseResult::new(vec![], errors));
    }

    #[test]
    fn resolve_and_type_results_round_trip() {
        let mut resolutions = HashMap::new();
        resolutions.insert(NodeId::new(0), DefId::new(1));
        let resolve = ResolveResult::new(
            resolutions,
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            vec![],
        );
        let json = serde_json::to_string(&resolve).expect("ser");
        let back: ResolveResult = serde_json::from_str(&json).expect("de");
        assert_eq!(back, resolve);

        let mut node_types = HashMap::new();
        node_types.insert(NodeId::new(0), Ty::Int);
        let types = TypeResult::new(node_types, vec![]);
        let json = serde_json::to_string(&types).expect("ser");
        let back: TypeResult = serde_json::from_str(&json).expect("de");
        assert_eq!(back, types);
    }
}
