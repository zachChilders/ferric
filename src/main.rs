use ferric_common::{Interner, Program, LexResult, ParseResult, ResolveResult, TypeResult, Symbol};
use ferric_lexer::lex;
use ferric_parser::parse;
use ferric_resolve::resolve_with_natives;
use ferric_typecheck::typecheck;
use ferric_vm::{Executor, TreeWalker, Value};
use ferric_stdlib::{NativeRegistry, register_stdlib};
use ferric_diagnostics::Renderer;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        // No arguments - start REPL
        run_repl();
    } else if args.len() == 2 {
        // One argument - run file
        run_file(&args[1]);
    } else {
        eprintln!("Usage: ferric [file]");
        eprintln!("  ferric          Start interactive REPL");
        eprintln!("  ferric <file>   Run a Ferric source file");
        process::exit(2);
    }
}

fn run_file(filename: &str) {
    let source = fs::read_to_string(filename)
        .unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        });

    // Create interner
    let mut interner = Interner::new();

    // Register stdlib BEFORE lexing so symbol IDs match
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);

    // Collect native function symbols for the resolver
    let native_symbols: Vec<Symbol> = vec![
        interner.intern("println"),
        interner.intern("print"),
        interner.intern("int_to_str"),
    ];

    // Lex
    let lex_result = lex(&source, &mut interner);

    // Parse
    let parse_result = parse(&lex_result);

    // Resolve (with knowledge of native functions)
    let resolve_result = resolve_with_natives(&parse_result, &native_symbols);

    // Type check
    let type_result = typecheck(&parse_result, &resolve_result, &interner);

    // Report errors
    if report_errors(&source, &lex_result, &parse_result, &resolve_result, &type_result) {
        process::exit(1);
    }

    // Create VM
    let mut vm: Box<dyn Executor> = Box::new(TreeWalker::new());

    // Create program (M1: just wrap the AST)
    let program = Program::new(parse_result.items.clone());

    // Execute
    match vm.run(program, natives, &interner) {
        Ok(_) => {
            // Success
            process::exit(0);
        }
        Err(e) => {
            let renderer = Renderer::new(source);
            eprintln!("{}", renderer.render_runtime_error(&e));
            process::exit(1);
        }
    }
}

fn run_repl() {
    use std::io::{self, Write};

    println!("Ferric REPL v0.1.0");
    println!("Type expressions to evaluate, or 'exit' to quit");
    println!();

    loop {
        print!(">> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            println!("Goodbye!");
            break;
        }

        // Fresh state for each evaluation
        let mut interner = Interner::new();
        let mut natives = NativeRegistry::new();
        register_stdlib(&mut natives, &mut interner);

        let native_symbols: Vec<Symbol> = vec![
            interner.intern("println"),
            interner.intern("print"),
            interner.intern("int_to_str"),
        ];

        // Parse and evaluate the input
        let lex_result = lex(input, &mut interner);

        if !lex_result.errors.is_empty() {
            for error in &lex_result.errors {
                let renderer = Renderer::new(input.to_string());
                eprintln!("{}", renderer.render_lex_error(error));
            }
            continue;
        }

        let parse_result = parse(&lex_result);

        if !parse_result.errors.is_empty() {
            for error in &parse_result.errors {
                let renderer = Renderer::new(input.to_string());
                eprintln!("{}", renderer.render_parse_error(error));
            }
            continue;
        }

        let resolve_result = resolve_with_natives(&parse_result, &native_symbols);

        if !resolve_result.errors.is_empty() {
            for error in &resolve_result.errors {
                let renderer = Renderer::new(input.to_string());
                eprintln!("{}", renderer.render_resolve_error(error));
            }
            continue;
        }

        let type_result = typecheck(&parse_result, &resolve_result, &interner);

        if !type_result.errors.is_empty() {
            for error in &type_result.errors {
                let renderer = Renderer::new(input.to_string());
                eprintln!("{}", renderer.render_type_error(error));
            }
            continue;
        }

        // Execute
        let program = Program::new(parse_result.items.clone());
        let mut vm: Box<dyn Executor> = Box::new(TreeWalker::new());

        match vm.run(program, natives, &interner) {
            Ok(value) => {
                // Print the result (except Unit)
                match value {
                    Value::Unit => {},
                    Value::Int(n) => println!("{}", n),
                    Value::Float(f) => println!("{}", f),
                    Value::Bool(b) => println!("{}", b),
                    Value::Str(s) => println!("\"{}\"", s),
                    Value::Fn(_) => println!("<function>"),
                }
            }
            Err(e) => {
                let renderer = Renderer::new(input.to_string());
                eprintln!("{}", renderer.render_runtime_error(&e));
            }
        }
    }
}

fn report_errors(
    source: &str,
    lex: &LexResult,
    parse: &ParseResult,
    resolve: &ResolveResult,
    types: &TypeResult,
) -> bool {
    let renderer = Renderer::new(source.to_string());
    let mut has_errors = false;

    for error in &lex.errors {
        eprintln!("{}", renderer.render_lex_error(error));
        has_errors = true;
    }

    for error in &parse.errors {
        eprintln!("{}", renderer.render_parse_error(error));
        has_errors = true;
    }

    for error in &resolve.errors {
        eprintln!("{}", renderer.render_resolve_error(error));
        has_errors = true;
    }

    for error in &types.errors {
        eprintln!("{}", renderer.render_type_error(error));
        has_errors = true;
    }

    has_errors
}
