const { workspace, window } = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');
const path = require('path');

let client;

function activate(context) {
    // FERRIC_LSP_PATH override lets you point at a `target/debug/ferric-lsp`
    // build during extension development. Without it, the extension runs the
    // release binary bundled at package time inside `bin/`.
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
