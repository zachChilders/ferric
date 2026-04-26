#!/usr/bin/env bash
# Build the LSP server, copy it into the VS Code extension folder, and produce
# a .vsix package at the repo root. Requires Node.js (vsce is invoked via npx).
#
# `cargo build` alone does not need any of this — the extension package is a
# release artefact, not a build artefact.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT_DIR="$REPO_ROOT/crates/ferric_lsp/vscode-extension"

# The Cargo *package* is `ferric_lsp` (snake_case to match workspace style);
# the *binary* it produces is `ferric-lsp` (hyphenated, what the extension
# launches). `--package` takes the package name.
echo "==> Building ferric_lsp (release)..."
cargo build --release --package ferric_lsp

# build.rs has already generated the TextMate grammar at this point.
GRAMMAR="$EXT_DIR/syntaxes/ferric.tmLanguage.json"
if [[ ! -f "$GRAMMAR" ]]; then
    echo "ERROR: $GRAMMAR missing — did cargo build run build.rs?" >&2
    exit 1
fi

echo "==> Copying LSP binary into extension..."
mkdir -p "$EXT_DIR/bin"
cp "$REPO_ROOT/target/release/ferric-lsp" "$EXT_DIR/bin/ferric-lsp"

echo "==> Installing extension dependencies..."
cd "$EXT_DIR"
npm install

echo "==> Packaging .vsix..."
npx --yes @vscode/vsce package --out "$REPO_ROOT/ferric-lang.vsix"

echo ""
echo "Done. Install with:"
echo "  code --install-extension $REPO_ROOT/ferric-lang.vsix"
