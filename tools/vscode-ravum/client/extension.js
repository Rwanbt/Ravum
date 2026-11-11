// Ravum VS Code client.
//
// The client is deliberately thin: every language feature comes from the real
// `ravum-lang lsp` process. Nothing is re-implemented here, so what this
// extension shows in VS Code is exactly what the compiler's analysis produced.
"use strict";

const fs = require("fs");
const path = require("path");
const vscode = require("vscode");

/** @type {import("vscode-languageclient/node").LanguageClient | undefined} */
let client;

function firstExisting(candidates) {
  for (const candidate of candidates) {
    try {
      if (candidate && fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
        return candidate;
      }
    } catch {
      // Unreadable path: not a candidate.
    }
  }
  return undefined;
}

function workspaceFolderPath() {
  const folders = vscode.workspace.workspaceFolders;
  return folders && folders.length > 0 ? folders[0].uri.fsPath : undefined;
}

/** Resolution order: setting, env var, workspace build output, PATH. */
function resolveServerPath() {
  const configured = vscode.workspace.getConfiguration("ravum.server").get("path", "");
  if (configured) {
    return configured;
  }
  if (process.env.RAVUM_LSP_BIN) {
    return process.env.RAVUM_LSP_BIN;
  }
  const root = workspaceFolderPath();
  if (root) {
    // Walk upwards so a nested package still finds the workspace target dir.
    let current = root;
    for (let depth = 0; depth < 8; depth += 1) {
      const found = firstExisting([
        path.join(current, "target", "debug", "ravum-lang.exe"),
        path.join(current, "target", "debug", "ravum-lang"),
        path.join(current, "target", "release", "ravum-lang.exe"),
        path.join(current, "target", "release", "ravum-lang"),
      ]);
      if (found) {
        return found;
      }
      const parent = path.dirname(current);
      if (parent === current) {
        break;
      }
      current = parent;
    }
  }
  return "ravum-lang";
}

function serverOptions() {
  const binary = resolveServerPath();
  const args = vscode.workspace.getConfiguration("ravum.server").get("args", ["lsp"]);
  return {
    command: binary,
    args,
    options: { cwd: workspaceFolderPath() },
  };
}

function clientOptions() {
  return {
    documentSelector: [{ scheme: "file", language: "ravum" }],
    synchronize: {
      // The server re-reads `ravum.toml`; no file watcher is needed for it.
      fileEvents: [],
    },
    outputChannelName: "Ravum Language Server",
    traceOutputChannelName: "Ravum LSP Trace",
  };
}

async function activate(context) {
  const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
  const server = serverOptions();
  server.options = { ...(server.options || {}) };
  void TransportKind;

  client = new LanguageClient(
    "ravum",
    "Ravum Language Server",
    server,
    clientOptions()
  );

  try {
    await client.start();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    void vscode.window.showErrorMessage(
      `Ravum: impossible de démarrer le serveur de langage (${server.command}). ${message}`
    );
    throw error;
  }

  context.subscriptions.push({
    dispose: () => {
      void client?.stop();
    },
  });

  // A command that surfaces what the server reports, so a user (or a test) can
  // confirm the server is alive beyond a single feature.
  context.subscriptions.push(
    vscode.commands.registerCommand("ravum.restartServer", async () => {
      if (client) {
        await client.stop();
        await client.start();
      }
    })
  );

  return { serverCommand: server.command, serverArgs: server.args };
}

async function deactivate() {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

module.exports = { activate, deactivate };
