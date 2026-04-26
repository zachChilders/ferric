.PHONY: build test lsp extension clean

build:
	cargo build

test:
	cargo test

# `ferric_lsp` is the Cargo package name; the binary it produces is
# `ferric-lsp`. `--package` takes the package name.
lsp:
	cargo build --package ferric_lsp

# Requires Node.js. vsce is invoked via npx.
extension:
	./tools/package-extension.sh

clean:
	cargo clean
	rm -f ferric-lang.vsix
	rm -rf crates/ferric_lsp/vscode-extension/bin
	rm -rf crates/ferric_lsp/vscode-extension/node_modules
