# Task: M1 CLI Implementation

## Objective
Implement the main CLI in `src/main.rs` that wires all pipeline stages together. This is the only file that knows about all stages and calls them in sequence.

## Architecture Context
- `main.rs` is the orchestrator - it imports and calls all stages
- It's the only place that depends on all crates
- When stages are replaced, only this file changes
- All other crates remain independent

## Public Interface

```rust
// src/main.rs
// Binary crate - no library interface

fn main() {
    // Parse command line args
    // Read source file
    // Run pipeline stages
    // Print errors or results
}
```

## Feature Requirements

### Command Line Interface (M1)
```
ferric <file>
```

Runs the Ferric interpreter on the given source file.

### Pipeline Orchestration

Wire all stages together in order:

1. **Read source file**
   - Check file exists
   - Read entire file to string

2. **Create interner**
   - `let mut interner = Interner::new();`

3. **Lex**
   - `let lex_result = ferric_lexer::lex(&source, &mut interner);`
   - If errors, render and exit

4. **Parse**
   - `let parse_result = ferric_parser::parse(&lex_result);`
   - If errors, render and exit

5. **Resolve**
   - `let resolve_result = ferric_resolve::resolve(&parse_result);`
   - If errors, render and exit

6. **Type check**
   - `let type_result = ferric_typecheck::typecheck(&parse_result, &resolve_result);`
   - If errors, render and exit

7. **Create VM and execute**
   - `let mut vm = TreeWalker::new();`
   - Create native registry: `let mut natives = NativeRegistry::new();`
   - Register stdlib: `ferric_stdlib::register_stdlib(&mut natives, &mut interner);`
   - Create program: `let program = Program { items: parse_result.items.clone() };`
   - Execute: `vm.run(program, natives)`
   - If error, render and exit
   - Otherwise, program completed successfully

8. **Error rendering**
   - Create `Renderer` with source
   - Render all errors from all stages
   - Print to stderr
   - Exit with code 1 if any errors

### Error Reporting
```rust
fn report_errors(source: &str,
                 lex: &LexResult,
                 parse: &ParseResult,
                 resolve: &ResolveResult,
                 types: &TypeResult) -> bool {
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
```

### Exit Codes
- 0: Success
- 1: Compilation or runtime errors
- 2: Usage error (wrong args)

## Implementation Notes

### Main Function Structure
```rust
use ferric_common::*;
use ferric_lexer;
use ferric_parser;
use ferric_resolve;
use ferric_typecheck;
use ferric_vm::{Executor, TreeWalker, Program};
use ferric_stdlib::{NativeRegistry, register_stdlib};
use ferric_diagnostics::Renderer;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: ferric <file>");
        std::process::exit(2);
    }

    let filename = &args[1];
    let source = std::fs::read_to_string(filename)
        .unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {}", filename, e);
            std::process::exit(1);
        });

    // Create interner
    let mut interner = Interner::new();

    // Lex
    let lex_result = ferric_lexer::lex(&source, &mut interner);

    // Parse
    let parse_result = ferric_parser::parse(&lex_result);

    // Resolve
    let resolve_result = ferric_resolve::resolve(&parse_result);

    // Type check
    let type_result = ferric_typecheck::typecheck(&parse_result, &resolve_result);

    // Report errors
    if report_errors(&source, &lex_result, &parse_result, &resolve_result, &type_result) {
        std::process::exit(1);
    }

    // Create VM and natives
    let mut vm = TreeWalker::new();
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);

    // Create program (M1: just wrap the AST)
    let program = Program {
        items: parse_result.items.clone(),
    };

    // Execute
    match vm.run(program, natives) {
        Ok(_) => {
            // Success
            std::process::exit(0);
        }
        Err(e) => {
            let renderer = Renderer::new(source);
            eprintln!("{}", renderer.render_runtime_error(&e));
            std::process::exit(1);
        }
    }
}

fn report_errors(...) { ... }
```

## M1 Test Program

Create a test file `examples/hello.fe`:
```rust
fn greet(name: Str) -> Str {
    "Hello, " + name
}

let message = greet("world")
println(message)
```

Running `ferric examples/hello.fe` should print:
```
Hello, world
```

## Acceptance Criteria
- [ ] CLI accepts a single filename argument
- [ ] All pipeline stages are called in correct order
- [ ] Interner is created and threaded through
- [ ] Errors from all stages are collected and rendered
- [ ] Native functions are registered before execution
- [ ] VM uses the Executor trait (not direct TreeWalker calls)
- [ ] Test program runs and produces correct output
- [ ] Undefined variable produces readable error and exits
- [ ] Type error produces readable error and exits
- [ ] Missing file produces error message

## Critical Rules to Enforce

### Rule 1 - Stages communicate only through output types
`main.rs` only imports stage entry functions and common types.
Never import internal stage types.

### Rule 2 - Each stage has exactly one public entry point
`main.rs` calls: `lex()`, `parse()`, `resolve()`, `typecheck()`, and `vm.run()`.
Nothing else.

### Rule 6 - VM is behind a trait
Create VM as `let mut vm: Box<dyn Executor> = Box::new(TreeWalker::new());`
or just use `TreeWalker` directly but call through `Executor::run()`.

## Notes for Agent
- This is the integration point - test end-to-end
- Make sure error reporting is clear and helpful
- Verify all stages are called in the right order
- Test with both valid and invalid programs
- Document the pipeline flow clearly
- This file will change when stages are replaced - that's expected and good
