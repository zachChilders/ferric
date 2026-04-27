# LSP — Task 3: TextMate grammar via build.rs

> **Prerequisite:** Task 1 complete (`ferric_common::keywords` exists).
> Task 2 must have created `crates/ferric_lsp/` (this task replaces its
> placeholder `build.rs`).

This task may run in parallel with tasks 04–06 once tasks 01 + 02 land.

---

## Goal

Replace the placeholder `build.rs` with a real script that generates
`crates/ferric_lsp/vscode-extension/syntaxes/ferric.tmLanguage.json` from
`ferric_common::keywords::{KEYWORDS, TYPE_KEYWORDS, OPERATORS}`. The grammar
file is **generated** — it is not committed to source control. Adding a keyword
to `KEYWORDS` and running `cargo build` automatically updates the grammar.

The script does not call `lex()`. It reads constant arrays only. There are no
runtime dependencies on the lexer.

---

## Files

### Replace — `crates/ferric_lsp/build.rs`

```rust
use ferric_common::keywords::{KEYWORDS, OPERATORS, TYPE_KEYWORDS};
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../ferric_common/src/keywords.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_path = manifest_dir
        .join("vscode-extension")
        .join("syntaxes")
        .join("ferric.tmLanguage.json");

    fs::create_dir_all(out_path.parent().unwrap())
        .expect("create syntaxes/ directory");

    let grammar = build_grammar();
    let json    = serde_json::to_string_pretty(&grammar).unwrap();
    fs::write(&out_path, json).expect("write tmLanguage.json");

    println!("cargo:warning=Generated TextMate grammar at {}", out_path.display());
}

fn build_grammar() -> Value {
    // \b anchors prevent `let` from matching inside `letter`.
    let kw_pattern = format!(r"\b({})\b", KEYWORDS.join("|"));
    let ty_pattern = format!(r"\b({})\b", TYPE_KEYWORDS.join("|"));

    // Operators are punctuation — no \b. Sort by length descending so that
    // multi-char operators (==, <=) match before single-char ones (=, <).
    let mut ops: Vec<&&str> = OPERATORS.iter().collect();
    ops.sort_by(|a, b| b.len().cmp(&a.len()));
    let op_pattern = ops
        .iter()
        .map(|op| regex_escape(op))
        .collect::<Vec<_>>()
        .join("|");

    json!({
        "$schema":   "https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json",
        "name":      "Ferric",
        "scopeName": "source.ferric",
        "fileTypes": ["fe"],
        "patterns": [
            { "include": "#comments" },
            { "include": "#strings" },
            { "include": "#shell-expressions" },
            { "include": "#booleans" },
            { "include": "#keywords" },
            { "include": "#type-keywords" },
            { "include": "#functions" },
            { "include": "#floats" },
            { "include": "#integers" },
            { "include": "#operators" }
        ],
        "repository": {
            "comments": {
                "name":  "comment.line.double-slash.ferric",
                "match": r"//.*$"
            },
            "strings": {
                "name":  "string.quoted.double.ferric",
                "begin": r#"""#,
                "end":   r#"""#,
                "patterns": [{
                    "name":  "constant.character.escape.ferric",
                    "match": r"\\."
                }]
            },
            "shell-expressions": {
                "comment": "Shell lines: $ command @{interp}",
                "begin":   r"(?<![a-zA-Z0-9_])\$\s",
                "end":     r"$",
                "name":    "meta.shell-expression.ferric",
                "beginCaptures": {
                    "0": { "name": "keyword.other.shell.ferric" }
                },
                "patterns": [{
                    "name":  "variable.other.interpolated.ferric",
                    "begin": r"@\{",
                    "end":   r"\}",
                    "patterns": [{ "include": "$self" }]
                }]
            },
            "keywords": {
                "name":  "keyword.control.ferric",
                "match": kw_pattern
            },
            "type-keywords": {
                "name":  "storage.type.ferric",
                "match": ty_pattern
            },
            "booleans": {
                "name":  "constant.language.boolean.ferric",
                "match": r"\b(true|false)\b"
            },
            "functions": {
                "comment": "Highlight the name in `fn name(`",
                "match":   r"\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*(?=\()",
                "captures": {
                    "1": { "name": "entity.name.function.ferric" }
                }
            },
            "floats": {
                "name":  "constant.numeric.float.ferric",
                "match": r"\b[0-9]+\.[0-9]+\b"
            },
            "integers": {
                "name":  "constant.numeric.integer.ferric",
                "match": r"\b[0-9]+\b"
            },
            "operators": {
                "name":  "keyword.operator.ferric",
                "match": op_pattern
            }
        }
    })
}

fn regex_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}'
            | '\\' | '^' | '$' | '|' => format!("\\{c}"),
            c => c.to_string(),
        })
        .collect()
}
```

> **Note on raw string literals:** `r"..."` and `r#"..."#` syntax is used above
> for regex patterns to avoid double escaping. Some patterns above use `r"\\."`
> (Rust raw string containing the four characters `\`, `\`, `.` — which becomes
> the regex `\\.` after JSON encoding's pass through `serde_json`). When in
> doubt, write the test in `Done when` below.

### Modify — `crates/ferric_lsp/.gitignore`

Create the file (or add to it if it exists):

```
vscode-extension/syntaxes/ferric.tmLanguage.json
```

The grammar is generated. The repo never tracks it.

---

## Done when

- [ ] Running `cargo build -p ferric-lsp` produces
      `crates/ferric_lsp/vscode-extension/syntaxes/ferric.tmLanguage.json`
- [ ] The generated file parses as valid JSON (`jq . file.json` succeeds)
- [ ] The generated file contains every string in `KEYWORDS` inside the
      `keywords` repository's `match` pattern
- [ ] The generated file contains every string in `TYPE_KEYWORDS` inside
      `type-keywords`
- [ ] The generated file contains every operator in `OPERATORS`, with
      multi-character operators ordered before single-character ones in the
      pattern (so `==` matches before `=`)
- [ ] Adding a fictional keyword to `ferric_common::keywords::KEYWORDS` (then
      reverting) and running `cargo build` regenerates the grammar with the
      new keyword
- [ ] `cargo:rerun-if-changed` is set on `../ferric_common/src/keywords.rs`
- [ ] `vscode-extension/syntaxes/ferric.tmLanguage.json` is in `.gitignore`
      and not committed
- [ ] No external tools (Node, npm, vsce) are required to run `cargo build`
