//! Generates `vscode-extension/syntaxes/ferric.tmLanguage.json` from
//! `ferric_common::keywords::{KEYWORDS, TYPE_KEYWORDS, OPERATORS}`.
//!
//! The grammar is a build artefact, not a source file — it is `.gitignore`d.
//! Adding a keyword to `ferric_common` automatically updates the grammar on
//! the next `cargo build`. No external tools (Node, npm, vsce) are required.

use ferric_common::keywords::{KEYWORDS, OPERATORS, TYPE_KEYWORDS};
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};

fn main() {
    // Re-run when the keyword list or this script change. Anything else can
    // remain cached.
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
    let json = serde_json::to_string_pretty(&grammar).expect("serialise grammar");
    fs::write(&out_path, json).expect("write tmLanguage.json");

    println!(
        "cargo:warning=ferric_lsp: regenerated TextMate grammar at {}",
        out_path.display()
    );
}

fn build_grammar() -> Value {
    // \b anchors prevent `let` from matching inside `letter`.
    let kw_pattern = format!(r"\b({})\b", KEYWORDS.join("|"));
    let ty_pattern = format!(r"\b({})\b", TYPE_KEYWORDS.join("|"));

    // Operators are punctuation — no word boundaries. Sort by length
    // descending so multi-character operators (`==`, `<=`, `&&`) match before
    // their single-character prefixes (`=`, `<`).
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
                "begin": "\"",
                "end":   "\"",
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
            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' | '^' | '$' | '|' => {
                format!("\\{c}")
            }
            c => c.to_string(),
        })
        .collect()
}
