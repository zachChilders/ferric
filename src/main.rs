use ferric_common::{
    ExhaustivenessError, ExhaustivenessResult, Interner, LexResult, ParseResult,
    ResolveResult, Symbol, TypeResult,
};
use ferric_lexer::lex;
use ferric_parser::parse_with_interner;
use ferric_resolve::resolve_with_natives;
use ferric_infer::typecheck;
use ferric_traits::build_registry;
use ferric_exhaust::check_exhaustiveness;
use ferric_vm::{BytecodeVM, Executor, Value};
use ferric_stdlib::{NativeRegistry, register_stdlib};
use ferric_diagnostics::Renderer;
use std::env;
use std::fs;
use std::process;

/// Table of `(fn_name, param_names)` for every native registered via
/// `register_stdlib`. The resolver uses parameter names to canonicalize
/// named-arg call sites.
fn native_fn_table(interner: &mut Interner) -> Vec<(Symbol, Vec<Symbol>)> {
    let mut intern = |name: &str| interner.intern(name);
    let entries: &[(&str, &[&str])] = &[
        ("println",         &["s"]),
        ("print",           &["s"]),
        ("int_to_str",      &["n"]),
        ("float_to_str",    &["n"]),
        ("bool_to_str",     &["b"]),
        ("int_to_float",    &["n"]),
        ("shell_stdout",    &["output"]),
        ("shell_exit_code", &["output"]),
        // M6
        ("array_len",       &["arr"]),
        ("str_len",         &["s"]),
        ("str_trim",        &["s"]),
        ("str_contains",    &["s", "sub"]),
        ("str_starts_with", &["s", "prefix"]),
        ("str_parse_int",   &["s"]),
        ("str_split",       &["s", "sep"]),
        ("abs",             &["n"]),
        ("min",             &["a", "b"]),
        ("max",             &["a", "b"]),
        ("sqrt",            &["n"]),
        ("pow",             &["base", "exp"]),
        ("floor",           &["n"]),
        ("ceil",            &["n"]),
        ("read_line",       &[]),
    ];
    entries
        .iter()
        .map(|(name, params)| {
            let n = intern(name);
            let ps = params.iter().map(|p| intern(p)).collect();
            (n, ps)
        })
        .collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // --dump-ast takes priority and bypasses the rest of the pipeline. It must
    // appear as the first argument; any subsequent positional argument is
    // treated as the source file.
    if args.len() == 3 && args[1] == "--dump-ast" {
        dump_ast(&args[2]);
        return;
    }

    if args.len() == 1 {
        // No arguments - start REPL
        run_repl();
    } else if args.len() == 2 {
        // One argument - run file
        run_file(&args[1]);
    } else {
        eprintln!("Usage: ferric [file]");
        eprintln!("  ferric                       Start interactive REPL");
        eprintln!("  ferric <file>                Run a Ferric source file");
        eprintln!("  ferric --dump-ast <file>     Print the parsed AST as JSON");
        process::exit(2);
    }
}

/// Runs the lexer + parser only and prints the AST as pretty-printed JSON.
///
/// External tools (LSPs, formatters, linters) consume Ferric's AST through
/// this entry point. No later stage runs.
fn dump_ast(filename: &str) {
    let source = fs::read_to_string(filename)
        .unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        });

    let mut interner = Interner::new();
    let lex_result = lex(&source, &mut interner);
    let parse_result = parse_with_interner(&lex_result, &interner);

    match ferric_common::ast_to_json(&parse_result) {
        Ok(json) => {
            println!("{}", json);
        }
        Err(e) => {
            eprintln!("Error serialising AST: {}", e);
            process::exit(1);
        }
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

    let native_fns = native_fn_table(&mut interner);

    // Lex
    let lex_result = lex(&source, &mut interner);

    // Parse
    let parse_result = parse_with_interner(&lex_result, &interner);

    // Resolve (with knowledge of native functions and their param names)
    let resolve_result = resolve_with_natives(&parse_result, &native_fns);

    // Build trait registry (M5).
    let trait_registry = build_registry(&parse_result, &resolve_result, &interner);

    // Type check
    let type_result = typecheck(&parse_result, &resolve_result, &interner, &trait_registry);

    // Exhaustiveness check (M4 — runs even if earlier stages had errors so that
    // pattern-related issues are reported alongside type errors).
    let exhaust_result = check_exhaustiveness(&parse_result, &type_result);

    // Report errors
    if report_errors(
        &source,
        &interner,
        &lex_result,
        &parse_result,
        &resolve_result,
        &type_result,
        &exhaust_result,
    ) {
        process::exit(1);
    }

    // Compile to bytecode.
    let program = ferric_compiler::compile(&parse_result, &resolve_result, &type_result, &interner);

    // Create VM
    let mut vm: Box<dyn Executor> = Box::new(BytecodeVM::new());

    // Execute
    match vm.run(program, natives, &interner) {
        Ok(_) => {
            // Success
            process::exit(0);
        }
        Err(e) => {
            let renderer = Renderer::with_interner(source, &interner);
            eprintln!("{}", renderer.render_runtime_error(&e));
            process::exit(1);
        }
    }
}

fn run_repl() {
    use std::io::{self, Write};

    println!("Ferric REPL v1.0");
    println!("Type expressions to evaluate, or 'exit' to quit.");
    println!("Each new line is appended to the session — definitions persist.");
    println!();

    // Source accumulated across all inputs. Each iteration re-runs the full
    // session so prior definitions are visible — a simple correctness strategy
    // that avoids reimplementing incremental compilation.
    let mut session = String::new();

    loop {
        print!(">> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            println!("Goodbye!");
            break;
        }
        if trimmed == ":reset" {
            session.clear();
            println!("(session cleared)");
            continue;
        }

        // Tentatively append the new input and run the full session. If
        // anything fails we leave `session` unchanged so the user can fix
        // and retry without polluting state.
        let candidate = if session.is_empty() {
            input.to_string()
        } else {
            format!("{}\n{}", session.trim_end(), input)
        };

        let result = run_session(&candidate);
        match result {
            Ok(()) => {
                session = candidate;
            }
            Err(message) => {
                eprintln!("{}", message);
            }
        }
    }
}

/// Runs the full accumulated session source. Errors are returned as a rendered
/// diagnostic string. On success returns `Ok(())`. The whole session re-runs
/// each iteration — side effects from earlier inputs replay (a known cost of
/// this simple "persistent state" strategy).
fn run_session(source: &str) -> Result<(), String> {
    // The simplest "persistent state" semantics: re-run the whole session
    // every time. Side effects replay — that's a known cost of this design.
    // (Suppressing prior side effects would require span-aware op tagging,
    // which is post-M6 work.)
    let mut interner = Interner::new();
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);
    let native_fns = native_fn_table(&mut interner);

    let lex_result = lex(source, &mut interner);
    if !lex_result.errors.is_empty() {
        let r = Renderer::with_interner(source.to_string(), &interner);
        return Err(lex_result
            .errors
            .iter()
            .map(|e| r.render_lex_error(e))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let parse_result = parse_with_interner(&lex_result, &interner);
    if !parse_result.errors.is_empty() {
        let r = Renderer::with_interner(source.to_string(), &interner);
        return Err(parse_result
            .errors
            .iter()
            .map(|e| r.render_parse_error(e))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let resolve_result = resolve_with_natives(&parse_result, &native_fns);
    if !resolve_result.errors.is_empty() {
        let r = Renderer::with_interner(source.to_string(), &interner);
        return Err(resolve_result
            .errors
            .iter()
            .map(|e| r.render_resolve_error(e))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let trait_registry = build_registry(&parse_result, &resolve_result, &interner);
    let type_result = typecheck(&parse_result, &resolve_result, &interner, &trait_registry);
    if !type_result.errors.is_empty() {
        let r = Renderer::with_interner(source.to_string(), &interner);
        return Err(type_result
            .errors
            .iter()
            .map(|e| r.render_type_error(e))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let exhaust_result = check_exhaustiveness(&parse_result, &type_result);
    let mut fatal_exhaust = Vec::new();
    for e in &exhaust_result.errors {
        if matches!(e, ExhaustivenessError::NonExhaustive { .. }) {
            fatal_exhaust.push(e.clone());
        }
    }
    if !fatal_exhaust.is_empty() {
        let r = Renderer::with_interner(source.to_string(), &interner);
        return Err(fatal_exhaust
            .iter()
            .map(|e| r.render_exhaustiveness_error(e))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let program = ferric_compiler::compile(
        &parse_result,
        &resolve_result,
        &type_result,
        &interner,
    );
    let mut vm: Box<dyn Executor> = Box::new(BytecodeVM::new());
    match vm.run(program, natives, &interner) {
        Ok(value) => {
            print_repl_value(&value);
            Ok(())
        }
        Err(e) => {
            let r = Renderer::with_interner(source.to_string(), &interner);
            Err(r.render_runtime_error(&e))
        }
    }
}

fn print_repl_value(value: &Value) {
    match value {
        Value::Unit => {}
        Value::Int(n) => println!("{}", n),
        Value::Float(f) => println!("{}", f),
        Value::Bool(b) => println!("{}", b),
        Value::Str(s) => println!("{:?}", s),
        Value::Fn(_) | Value::NativeFn(_) => println!("<function>"),
        Value::ShellOutput(out) => {
            println!(
                "ShellOutput {{ exit_code: {}, stdout: {:?} }}",
                out.exit_code, out.stdout
            )
        }
        Value::Struct(fields) => println!("Struct({:?})", fields),
        Value::Variant(idx, fields) => println!("Variant({}, {:?})", idx, fields),
        Value::Tuple(elems) => println!("Tuple({:?})", elems),
        Value::Array(elems) => println!("{:?}", elems),
        Value::Closure { .. } => println!("<closure>"),
    }
}

fn report_errors(
    source: &str,
    interner: &Interner,
    lex: &LexResult,
    parse: &ParseResult,
    resolve: &ResolveResult,
    types: &TypeResult,
    exhaust: &ExhaustivenessResult,
) -> bool {
    let renderer = Renderer::with_interner(source.to_string(), interner);
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

    for error in &exhaust.errors {
        eprintln!("{}", renderer.render_exhaustiveness_error(error));
        // Unreachable arms are warnings, not errors — they don't fail the
        // build.
        if matches!(error, ExhaustivenessError::NonExhaustive { .. }) {
            has_errors = true;
        }
    }

    has_errors
}
