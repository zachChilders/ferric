use ferric_common::Interner;
use ferric_lexer::lex;
use ferric_parser::parse;
use ferric_resolve::resolve;
use std::env;
use std::fs;
use std::process;

fn main() {
    // Get filename from command line args
    let args: Vec<String> = env::args().collect();

    let filename = if args.len() > 1 {
        &args[1]
    } else {
        "examples/test.fe"
    };

    println!("=== Ferric Compiler - M1 Progress ===");
    println!("File: {}\n", filename);

    // Read the source file
    let source = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Error reading file '{}': {}", filename, err);
            eprintln!("\nUsage: cargo run [file.fe]");
            eprintln!("Example: cargo run examples/test.fe");
            process::exit(1);
        }
    };

    // Create an interner for string interning
    let mut interner = Interner::new();

    println!("Source code:");
    println!("{}", "=".repeat(60));
    println!("{}", source);
    println!("{}", "=".repeat(60));
    println!();

    // Lex the source
    let lex_result = lex(&source, &mut interner);

    println!("📝 Lexer Results:");
    println!("  Tokens: {}", lex_result.tokens.len());
    println!("  Errors: {}", lex_result.errors.len());

    if !lex_result.errors.is_empty() {
        println!("\n❌ Lexer Errors:");
        for err in &lex_result.errors {
            println!("  - {} at position {}", err.description(), err.span().start);
        }
        println!();
    } else {
        println!("  ✓ Lexing successful!");
        println!();
    }

    // Parse the tokens
    let parse_result = parse(&lex_result);

    println!("🌲 Parser Results:");
    println!("  Top-level items: {}", parse_result.items.len());
    println!("  Errors: {}", parse_result.errors.len());

    if !parse_result.errors.is_empty() {
        println!("\n❌ Parser Errors:");
        for err in &parse_result.errors {
            println!("  - {} at position {}", err.description(), err.span().start);
        }
        println!();
    } else {
        println!("  ✓ Parsing successful!");
        println!();
    }

    // Print AST summary
    if parse_result.errors.is_empty() && !parse_result.items.is_empty() {
        println!("📋 AST Summary:");

        let mut script_count = 0;
        let mut fn_count = 0;

        for item in &parse_result.items {
            match item {
                ferric_common::Item::FnDef { name, params, ret_ty, .. } => {
                    let name_str = interner.resolve(*name);
                    let param_count = params.len();
                    let ret_type = match ret_ty {
                        ferric_common::TypeAnnotation::Named(sym) => interner.resolve(*sym),
                    };
                    println!("  fn {}({} params) -> {}", name_str, param_count, ret_type);
                    fn_count += 1;
                }
                ferric_common::Item::Script { stmt, .. } => {
                    script_count += 1;
                    match stmt {
                        ferric_common::Stmt::Let { name, .. } => {
                            let name_str = interner.resolve(*name);
                            println!("  let {} = ...", name_str);
                        }
                        ferric_common::Stmt::Expr { .. } => {
                            println!("  <expression>");
                        }
                    }
                }
            }
        }

        println!();
        println!("  Summary: {} function(s), {} script statement(s)", fn_count, script_count);
        println!();
    }

    // Perform name resolution
    let resolve_result = resolve(&parse_result);

    println!("🔍 Name Resolution Results:");
    println!("  Resolutions: {}", resolve_result.resolutions.len());
    println!("  Variable slots: {}", resolve_result.def_slots.len());
    println!("  Function slots: {}", resolve_result.fn_slots.len());
    println!("  Errors: {}", resolve_result.errors.len());

    if !resolve_result.errors.is_empty() {
        println!("\n❌ Resolution Errors:");
        for err in &resolve_result.errors {
            println!("  - {} at position {}", err.description(), err.span().start);
        }
        println!();
    } else {
        println!("  ✓ Name resolution successful!");
        println!();
    }

    // Summary
    println!("=== Summary ===");
    if lex_result.errors.is_empty() && parse_result.errors.is_empty() && resolve_result.errors.is_empty() {
        println!("✅ All stages completed successfully!");
        println!("\n📌 Current pipeline:");
        println!("  ✓ Lexer: Tokenizes source code");
        println!("  ✓ Parser: Builds AST with proper precedence");
        println!("  ✓ Name Resolution: Maps variables to definitions");
        println!("  ⏳ Type Checking: Not yet implemented");
        println!("  ⏳ VM/Execution: Not yet implemented");
    } else {
        println!("❌ Compilation failed with {} error(s)",
                 lex_result.errors.len() + parse_result.errors.len() + resolve_result.errors.len());
        process::exit(1);
    }
}
