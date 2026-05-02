//! `ferric init-ai` — installs the Ferric language reference into AI assistant
//! config directories so tools like Claude Code, Cursor, and GitHub Copilot
//! have accurate syntax and stdlib knowledge.
//!
//! Usage:
//!   ferric init-ai            auto-detect installed tools and set up all found
//!   ferric init-ai all        set up every tool regardless of detection
//!   ferric init-ai claude     Claude Code only  (.claude/ + CLAUDE.md)
//!   ferric init-ai cursor     Cursor only        (.cursor/rules/ferric.mdc)
//!   ferric init-ai copilot    GitHub Copilot only (.github/copilot-instructions.md)

use std::fs;
use std::path::Path;

/// The language reference embedded at compile time so the binary is
/// self-contained — no network access or installation path required.
const LANGUAGE_MD: &str = include_str!("../language.md");

pub fn run(args: &[String]) {
    let targets = if args.is_empty() {
        detect_targets()
    } else {
        parse_targets(args)
    };

    if targets.is_empty() {
        println!("No AI assistant config detected in the current directory.");
        println!("Run with an explicit target to set one up:");
        println!("  ferric init-ai all");
        println!("  ferric init-ai claude|cursor|copilot");
        return;
    }

    for target in targets {
        match target {
            Target::Claude => setup_claude(),
            Target::Cursor => setup_cursor(),
            Target::Copilot => setup_copilot(),
        }
    }
}

enum Target {
    Claude,
    Cursor,
    Copilot,
}

/// Auto-detect which AI tools are configured in the current directory.
/// Returns all three if nothing is detected (first-time setup).
fn detect_targets() -> Vec<Target> {
    let has_claude = Path::new("CLAUDE.md").exists() || Path::new(".claude").is_dir();
    let has_cursor = Path::new(".cursor").is_dir() || Path::new(".cursorrules").exists();
    let has_copilot =
        Path::new(".github/copilot-instructions.md").exists() || Path::new(".github").is_dir();

    let mut targets = Vec::new();
    if has_claude {
        targets.push(Target::Claude);
    }
    if has_cursor {
        targets.push(Target::Cursor);
    }
    if has_copilot {
        targets.push(Target::Copilot);
    }

    // Nothing detected — set up everything so a fresh project gets coverage.
    if targets.is_empty() {
        targets = vec![Target::Claude, Target::Cursor, Target::Copilot];
    }

    targets
}

fn parse_targets(args: &[String]) -> Vec<Target> {
    let mut targets = Vec::new();
    for arg in args {
        match arg.as_str() {
            "claude" => targets.push(Target::Claude),
            "cursor" => targets.push(Target::Cursor),
            "copilot" => targets.push(Target::Copilot),
            "all" => return vec![Target::Claude, Target::Cursor, Target::Copilot],
            other => eprintln!(
                "warning: unknown target '{other}' (expected: claude, cursor, copilot, all)"
            ),
        }
    }
    targets
}

// ---------------------------------------------------------------------------
// Per-tool setup
// ---------------------------------------------------------------------------

/// Claude Code: `.claude/ferric-language.md` + `@` import in `CLAUDE.md`.
///
/// Claude Code loads `CLAUDE.md` on startup and follows `@path` directives
/// to include additional context files.
fn setup_claude() {
    let dir = ".claude";
    let dest = ".claude/ferric-language.md";
    let import_line = "@.claude/ferric-language.md";

    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!("claude: failed to create {dir}/: {e}");
        return;
    }

    if let Err(e) = fs::write(dest, LANGUAGE_MD) {
        eprintln!("claude: failed to write {dest}: {e}");
        return;
    }
    println!("claude: wrote {dest}");

    let claude_md = Path::new("CLAUDE.md");
    if claude_md.exists() {
        match fs::read_to_string(claude_md) {
            Ok(existing) if existing.contains(import_line) => {
                println!("claude: CLAUDE.md already imports ferric-language.md — skipped");
            }
            Ok(existing) => {
                // Append an import section so existing project instructions are
                // preserved and the language reference is clearly separated.
                let updated =
                    format!("{existing}\n## Ferric language reference\n\n{import_line}\n");
                if let Err(e) = fs::write(claude_md, updated) {
                    eprintln!("claude: failed to update CLAUDE.md: {e}");
                } else {
                    println!("claude: appended import to CLAUDE.md");
                }
            }
            Err(e) => eprintln!("claude: failed to read CLAUDE.md: {e}"),
        }
    } else {
        let content = format!("## Ferric language reference\n\n{import_line}\n");
        if let Err(e) = fs::write(claude_md, content) {
            eprintln!("claude: failed to create CLAUDE.md: {e}");
        } else {
            println!("claude: created CLAUDE.md");
        }
    }
}

/// Cursor: `.cursor/rules/ferric.mdc` with frontmatter.
///
/// Cursor loads `.mdc` files from `.cursor/rules/` and makes them available
/// as agent rules. The `globs` field scopes the rule to `.fe` files;
/// `alwaysApply: false` means Cursor attaches it on demand.
fn setup_cursor() {
    let rules_dir = ".cursor/rules";
    let dest = ".cursor/rules/ferric.mdc";

    if let Err(e) = fs::create_dir_all(rules_dir) {
        eprintln!("cursor: failed to create {rules_dir}/: {e}");
        return;
    }

    let content = format!(
        "---\n\
         description: Ferric language syntax and semantics\n\
         globs: [\"**/*.fe\"]\n\
         alwaysApply: false\n\
         ---\n\
         \n\
         {LANGUAGE_MD}"
    );

    if let Err(e) = fs::write(dest, &content) {
        eprintln!("cursor: failed to write {dest}: {e}");
        return;
    }
    println!("cursor: wrote {dest}");
}

/// GitHub Copilot: `.github/copilot-instructions.md`.
///
/// Copilot loads this file as repository-level instructions for all chat
/// and inline completions. If the file already exists the language reference
/// is prepended so Ferric context comes first.
fn setup_copilot() {
    let github_dir = ".github";
    let dest = ".github/copilot-instructions.md";

    if let Err(e) = fs::create_dir_all(github_dir) {
        eprintln!("copilot: failed to create {github_dir}/: {e}");
        return;
    }

    let copilot_path = Path::new(dest);
    if copilot_path.exists() {
        match fs::read_to_string(copilot_path) {
            Ok(existing) if existing.contains("# Ferric Language Reference") => {
                println!("copilot: {dest} already contains Ferric reference — skipped");
            }
            Ok(existing) => {
                let updated = format!("{LANGUAGE_MD}\n\n---\n\n{existing}");
                if let Err(e) = fs::write(copilot_path, updated) {
                    eprintln!("copilot: failed to update {dest}: {e}");
                } else {
                    println!("copilot: prepended Ferric reference to existing {dest}");
                }
            }
            Err(e) => eprintln!("copilot: failed to read {dest}: {e}"),
        }
    } else {
        if let Err(e) = fs::write(copilot_path, LANGUAGE_MD) {
            eprintln!("copilot: failed to write {dest}: {e}");
            return;
        }
        println!("copilot: wrote {dest}");
    }
}
