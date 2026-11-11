// Integration suite executed by a real Visual Studio Code instance.
//
// Every assertion below goes through VS Code's public API, not through the
// language server directly: `vscode.executeHoverProvider` and friends ask
// whichever provider is registered for `ravum`. That provider is the real
// `ravum-lang lsp` process started by this extension, so a green run proves the
// whole chain editor -> LSP client -> `ravum-lang lsp` -> checker -> editor.
"use strict";

const assert = require("assert");
const path = require("path");
const vscode = require("vscode");

const workspace = process.env.RAVUM_E2E_WORKSPACE;
assert.ok(workspace, "RAVUM_E2E_WORKSPACE must be set by test/runTest.js");
const mainPath = path.join(workspace, "src", "main.rv");

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(label, predicate, timeoutMs = 180000, intervalMs = 250) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await predicate();
    if (value) {
      return value;
    }
    await sleep(intervalMs);
  }
  throw new Error(`timed out after ${timeoutMs}ms waiting for: ${label}`);
}

function offsetOfNth(text, needle, n = 1) {
  let index = -1;
  for (let seen = 0; seen < n; seen += 1) {
    index = text.indexOf(needle, index + 1);
    if (index === -1) {
      throw new Error(`occurrence ${n} of ${JSON.stringify(needle)} not found`);
    }
  }
  return index;
}

function positionOf(document, needle, n = 1) {
  return document.positionAt(offsetOfNth(document.getText(), needle, n));
}

// `vscode.Position` is immutable: move the cursor with a new instance.
function shifted(position, characters) {
  return new vscode.Position(position.line, position.character + characters);
}

function hoverText(hovers) {
  const parts = [];
  for (const hover of hovers || []) {
    for (const content of hover.contents || []) {
      if (typeof content === "string") {
        parts.push(content);
      } else if (content && typeof content.value === "string") {
        parts.push(content.value);
      }
    }
  }
  return parts.join("\n");
}

// `vscode.WorkspaceEdit` exposes neither `changes` nor `documentChanges`: the
// only public accessors are `size` and `entries()`. Reading the LSP-shaped
// properties directly silently yields zero edits, which is exactly how a
// working rename can look broken from a test.
function rangesOfEdit(edit) {
  const ranges = [];
  if (!edit) {
    return ranges;
  }
  if (typeof edit.entries === "function") {
    for (const [, edits] of edit.entries()) {
      for (const change of edits) {
        if (change.range) {
          ranges.push(change.range);
        }
      }
    }
    return ranges;
  }
  return ranges;
}

describe("Ravum language support inside a real VS Code session", () => {
  let document;
  let editor;

  before(async () => {
    const extension = vscode.extensions.getExtension("ravum.ravum-lang");
    assert.ok(extension, "the Ravum extension is installed in this VS Code instance");
    const exports = extension.isActive ? extension.exports : await extension.activate();
    assert.ok(exports, "activate() returned the client descriptor");
    assert.ok(
      /ravum-lang(\.exe)?$/.test(exports.serverCommand),
      `the server command is the real ravum-lang binary, got ${exports.serverCommand}`
    );
    assert.deepStrictEqual(exports.serverArgs, ["lsp"]);

    document = await vscode.workspace.openTextDocument(vscode.Uri.file(mainPath));
    editor = await vscode.window.showTextDocument(document);

    // Readiness barrier: prove once, with a generous budget, that the language
    // server is answering before the timed assertions start. On a loaded
    // machine the first analysis of the workspace can take minutes; failing a
    // *feature* test on that startup latency reported a false defect (T7).
    await waitFor(
      "the ravum-lang server to answer documentSymbol",
      async () => {
        const found = await vscode.commands.executeCommand(
          "vscode.executeDocumentSymbolProvider",
          document.uri
        );
        return found && found.length > 0 ? found : undefined;
      },
      240000,
      500
    );
  });

  it("opens the package as a Ravum buffer with the syntax grammar", async () => {
    assert.strictEqual(document.languageId, "ravum");
    const grammars = vscode.extensions.getExtension("ravum.ravum-lang").packageJSON
      .contributes.grammars;
    assert.strictEqual(grammars[0].scopeName, "source.ravum");
    assert.strictEqual(grammars[0].language, "ravum");
  });

  it("starts with no error diagnostic on a valid program", async () => {
    // The first analysis can lag the didOpen: wait until the server has produced
    // either nothing or only non-error diagnostics, then assert on errors.
    await sleep(3000);
    const errors = vscode.languages
      .getDiagnostics(document.uri)
      .filter((diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error);
    assert.deepStrictEqual(
      errors.map((diagnostic) => `${diagnostic.code}: ${diagnostic.message}`),
      [],
      "a valid Core 1.0 program must not produce errors"
    );
  });

  it("returns the document symbols produced by the compiler", async () => {
    const symbols = await waitFor("document symbols for main.rv", async () => {
      const found = await vscode.commands.executeCommand(
        "vscode.executeDocumentSymbolProvider",
        document.uri
      );
      return found && found.length > 0 ? found : undefined;
    });
    const names = symbols.map((symbol) => symbol.name);
    assert.ok(names.includes("add"), `symbols contain add, got ${names.join(",")}`);
    assert.ok(names.includes("main"), `symbols contain main, got ${names.join(",")}`);
  });

  it("completes the package's own items and the language keywords", async () => {
    // An empty word at the cursor keeps VS Code from filtering the list by the
    // prefix, which would hide every item that does not start with `total`.
    const position = shifted(positionOf(document, "let total: i32 = "), "let total: i32 = ".length);
    const list = await vscode.commands.executeCommand(
      "vscode.executeCompletionItemProvider",
      document.uri,
      position
    );
    const labels = (list.items || []).map((item) =>
      typeof item.label === "string" ? item.label : item.label.label
    );
    assert.ok(labels.includes("add"), "completion offers the function declared in the file");
    assert.ok(labels.includes("fn"), "completion offers Ravum keywords");
  });

  it("shows the compiler's hover for a call site", async () => {
    const position = positionOf(document, "add(1, 2)");
    const hovers = await waitFor("hover on add(1, 2)", async () => {
      const found = await vscode.commands.executeCommand(
        "vscode.executeHoverProvider",
        document.uri,
        position
      );
      return found && found.length > 0 ? found : undefined;
    });
    const markdown = hoverText(hovers);
    assert.ok(markdown.includes("fn add"), `hover mentions the signature, got: ${markdown}`);
    assert.ok(markdown.includes("i32"), `hover mentions the types, got: ${markdown}`);
  });

  it("jumps to the declaration through go-to-definition", async () => {
    const position = positionOf(document, "add(1, 2)");
    const definitions = await waitFor("definition of add(1, 2)", async () => {
      const found = await vscode.commands.executeCommand(
        "vscode.executeDefinitionProvider",
        document.uri,
        position
      );
      return found && found.length > 0 ? found : undefined;
    });
    const first = definitions[0];
    const range = first.targetRange || first.range;
    const uri = first.targetUri || first.uri;
    assert.strictEqual(uri.fsPath.toLowerCase(), document.uri.fsPath.toLowerCase());
    const line = document.lineAt(range.start.line).text;
    assert.ok(line.includes("fn add"), `jumped to the declaration line, got: ${line}`);
  });

  it("finds every reference of the function", async () => {
    const position = shifted(positionOf(document, "fn add"), "fn ".length);
    const references = await waitFor("references of add", async () => {
      const found = await vscode.commands.executeCommand(
        "vscode.executeReferenceProvider",
        document.uri,
        position
      );
      return found && found.length >= 3 ? found : undefined;
    });
    assert.ok(
      references.length >= 3,
      `expected declaration + 2 calls, got ${references.length}`
    );
  });

  it("formats the buffer with the compiler's formatter", async () => {
    const original = document.getText();
    const mangled = original.replace(
      "fn add(a: i32, b: i32) -> i32 {",
      "fn   add( a :i32,b : i32 )->i32 {"
    );
    assert.notStrictEqual(mangled, original, "the mangled source differs");
    const full = new vscode.Range(
      new vscode.Position(0, 0),
      document.positionAt(original.length)
    );
    assert.ok(
      await editor.edit((builder) => builder.replace(full, mangled)),
      "mangled source written to the buffer"
    );
    assert.ok(document.getText().includes("fn   add"), "buffer holds the mangled source");

    const edits = await waitFor("formatting edits for a messy buffer", async () => {
      const found = await vscode.commands.executeCommand(
        "vscode.executeFormatDocumentProvider",
        document.uri,
        { tabSize: 4, insertSpaces: true }
      );
      return found && found.length > 0 ? found : undefined;
    });

    const workspaceEdit = new vscode.WorkspaceEdit();
    for (const edit of edits) {
      workspaceEdit.replace(document.uri, edit.range, edit.newText);
    }
    assert.ok(await vscode.workspace.applyEdit(workspaceEdit), "formatter edits applied");
    const formatted = document.getText();
    assert.ok(
      formatted.includes("fn add(a: i32, b: i32) -> i32 {"),
      `formatter restored the canonical signature, got: ${formatted.split("\n")[0]}`
    );
    assert.ok(!formatted.includes("fn   add"), "the messy spacing is gone");
  });

  it("renames a symbol and every call site", async () => {
    const position = shifted(positionOf(document, "fn add"), "fn ".length);
    const edit = await waitFor("rename edit for add", async () => {
      const found = await vscode.commands.executeCommand(
        "vscode.executeDocumentRenameProvider",
        document.uri,
        position,
        "sum"
      );
      return found && (found.size >= 3 || rangesOfEdit(found).length >= 3) ? found : undefined;
    });
    const ranges = rangesOfEdit(edit);
    assert.ok(ranges.length >= 3, `rename touches declaration + 2 calls, got ${ranges.length}`);
    assert.ok(await vscode.workspace.applyEdit(edit), "rename applied to the workspace");
    const renamed = document.getText();
    assert.ok(renamed.includes("fn sum(a: i32, b: i32)"), "the declaration was renamed");
    assert.ok(renamed.includes("sum(1, 2)"), "the first call site was renamed");
    assert.ok(!renamed.includes("add("), "no call site keeps the old name");
  });

  it("publishes a compiler diagnostic for an invalid program", async () => {
    const text = document.getText();
    const line = text.split("\n").findIndex((line) => line.trim() === 'io::println("done");');
    assert.ok(line >= 0, "the insertion point exists");
    assert.ok(
      await editor.edit((builder) =>
        builder.insert(new vscode.Position(line, 0), '    let broken: i32 = "text";\n')
      ),
      "invalid statement inserted"
    );

    const diagnostics = await waitFor(
      "an error diagnostic published by the language server",
      async () => {
        const all = vscode.languages.getDiagnostics(document.uri);
        const errors = all.filter(
          (diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error
        );
        return errors.length > 0 ? errors : undefined;
      },
      120000
    );
    const summary = diagnostics
      .map((diagnostic) => `${diagnostic.source || "?"}:${diagnostic.code || "?"}:${diagnostic.message}`)
      .join(" | ");
    assert.ok(
      diagnostics.some((diagnostic) => String(diagnostic.code || "").startsWith("E-")),
      `a stable Ravum diagnostic code is exposed to the editor, got: ${summary}`
    );
    assert.ok(
      diagnostics.some((diagnostic) => /i32|string|str|type/i.test(diagnostic.message)),
      `the message names the type error, got: ${summary}`
    );
    assert.ok(
      diagnostics.every((diagnostic) => diagnostic.range.start.line >= 0),
      "diagnostics carry a usable range"
    );
  });
});
