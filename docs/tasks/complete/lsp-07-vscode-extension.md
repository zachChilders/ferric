# LSP — Task 7: VS Code Extension + Packaging

> **Prerequisite:** None for the file authoring; Task 2 + Task 3 must be
> complete before the end-to-end install test (the binary and the grammar need
> to exist).

---

## Goal

Add the VS Code extension package files (`package.json`, `language-configuration.json`,
`client/extension.js`), the packaging shell script, and the top-level
`Makefile`. After this task, running `make extension` produces a `.vsix` file
that, when installed in VS Code, gives the user syntax highlighting and a
running language server.

The extension is a thin transport wrapper. It has no knowledge of the Ferric
pipeline — it only spawns the `ferric-lsp` binary and connects to it over
stdio.

---

## Files

### Create — `crates/ferric_lsp/vscode-extension/package.json`

```json
{
    "name": "ferric-lang",
    "displayName": "Ferric",
    "description": "Language support for the Ferric programming language",
    "version": "0.1.0",
    "publisher": "ferric",
    "engines": { "vscode": "^1.75.0" },
    "categories": ["Programming Languages"],
    "main": "./client/extension.js",
    "contributes": {
        "languages": [{
            "id": "ferric",
            "aliases": ["Ferric", "ferric"],
            "extensions": [".fe"],
            "configuration": "./language-configuration.json"
        }],
        "grammars": [{
            "language": "ferric",
            "scopeName": "source.ferric",
            "path": "./syntaxes/ferric.tmLanguage.json"
        }]
    },
    "activationEvents": ["onLanguage:ferric"],
    "dependencies": {
        "vscode-languageclient": "^9.0.0"
    },
    "devDependencies": {
        "@types/node": "^20",
        "@types/vscode": "^1.75.0"
    }
}
```

### Create — `crates/ferric_lsp/vscode-extension/language-configuration.json`

```json
{
    "comments": {
        "lineComment": "//"
    },
    "brackets": [
        ["{", "}"],
        ["[", "]"],
        ["(", ")"]
    ],
    "autoClosingPairs": [
        { "open": "{",  "close": "}" },
        { "open": "[",  "close": "]" },
        { "open": "(",  "close": ")" },
        { "open": "\"", "close": "\"" }
    ],
    "surroundingPairs": [
        ["{", "}"], ["[", "]"], ["(", ")"], ["\"", "\""]
    ],
    "indentationRules": {
        "increaseIndentPattern": "\\{\\s*$",
        "decreaseIndentPattern": "^\\s*\\}"
    }
}
```

### Create — `crates/ferric_lsp/vscode-extension/client/extension.js`

```javascript
const { workspace, window } = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');
const path = require('path');

let client;

function activate(context) {
    const lspBinary = process.env.FERRIC_LSP_PATH
        || path.join(context.extensionPath, 'bin', 'ferric-lsp');

    const serverOptions = {
        run:   { command: lspBinary, transport: TransportKind.stdio },
        debug: {
            command:   lspBinary,
            transport: TransportKind.stdio,
            args:      ['--log-level', 'debug'],
        },
    };

    const clientOptions = {
        documentSelector: [{ scheme: 'file', language: 'ferric' }],
        synchronize: {
            fileEvents: workspace.createFileSystemWatcher('**/*.fe'),
        },
    };

    client = new LanguageClient(
        'ferric-lsp',
        'Ferric Language Server',
        serverOptions,
        clientOptions,
    );

    client.start().catch(err => {
        window.showErrorMessage(`Failed to start ferric-lsp: ${err.message}`);
    });
}

function deactivate() {
    return client?.stop();
}

module.exports = { activate, deactivate };
```

### Create — `crates/ferric_lsp/vscode-extension/.vscodeignore`

Tells `vsce package` which files NOT to include:

```
.vscode/**
.vscode-test/**
out/test/**
src/**
.gitignore
.eslintrc.json
**/tsconfig.json
**/*.map
**/*.ts
node_modules/.cache/**
```

### Create — `crates/ferric_lsp/vscode-extension/.gitignore`

```
node_modules/
bin/
*.vsix
syntaxes/ferric.tmLanguage.json
```

### Create — `tools/package-extension.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT_DIR="$REPO_ROOT/crates/ferric_lsp/vscode-extension"

echo "==> Building ferric-lsp (release)..."
cargo build --release --package ferric-lsp

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
```

Make executable:

```bash
chmod +x tools/package-extension.sh
```

### Create or modify — `Makefile` (top-level)

If a `Makefile` already exists, merge these targets in. Otherwise create:

```makefile
.PHONY: build test lsp extension clean

build:
	cargo build

test:
	cargo test

lsp:
	cargo build --package ferric-lsp

# Requires Node.js. vsce is invoked via npx.
extension:
	./tools/package-extension.sh

clean:
	cargo clean
	rm -f ferric-lang.vsix
	rm -rf crates/ferric_lsp/vscode-extension/bin
	rm -rf crates/ferric_lsp/vscode-extension/node_modules
```

---

## Done when

**File presence:**
- [ ] `crates/ferric_lsp/vscode-extension/package.json` exists and is valid JSON
- [ ] `language-configuration.json` exists with bracket pairs and indent rules
- [ ] `client/extension.js` exists and uses `vscode-languageclient/node`
- [ ] `.vscodeignore` and `.gitignore` exist
- [ ] `tools/package-extension.sh` exists and is `chmod +x`
- [ ] `Makefile` has targets `build`, `test`, `lsp`, `extension`, `clean`

**Behavior:**
- [ ] `make build` builds the workspace (no extension work involved)
- [ ] `make lsp` builds only `ferric-lsp`
- [ ] `make extension` runs `cargo build --release --package ferric-lsp`,
      copies the binary into `vscode-extension/bin/`, runs `npm install`, runs
      `vsce package`, and produces `ferric-lang.vsix` at the repo root
- [ ] `cargo build` alone (no extension target) succeeds without Node.js
      installed

**End-to-end (manual):**
- [ ] `code --install-extension ferric-lang.vsix` installs cleanly
- [ ] Opening a `.fe` file in VS Code shows syntax highlighting
- [ ] Opening a `.fe` file activates the extension and starts `ferric-lsp`
      (visible in VS Code's Output panel under "Ferric Language Server")
- [ ] Errors in the source file appear as red squigglies (diagnostics work)
- [ ] Hover, completion, goto-def, document symbols, inlay hints all work
      (assumes Tasks 04, 05, 06 are complete)
- [ ] Setting `FERRIC_LSP_PATH=$(pwd)/target/debug/ferric-lsp` and reloading
      the window uses the dev binary instead of the bundled release

**Architecture:**
- [ ] The extension's only knowledge of Ferric is via stdio LSP — it never
      imports anything Ferric-specific
- [ ] Removing `ferric-lsp` and rebuilding does not break `cargo build` for
      any other crate (the LSP is not a workspace dependency of the
      interpreter)
