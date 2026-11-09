// Launches a real Visual Studio Code instance with this extension installed
// (development mode) and runs the integration suite inside its extension host.
//
// Nothing about the language is simulated: VS Code starts `ravum-lang lsp` as a
// child process and the assertions go through VS Code's own language-feature
// pipeline (`vscode.executeHoverProvider`, diagnostics, rename, …).
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const { runTests } = require("@vscode/test-electron");

const SOURCE = [
  "fn add(a: i32, b: i32) -> i32 {",
  "    a + b",
  "}",
  "",
  "fn main() {",
  "    let total: i32 = add(1, 2);",
  "    let doubled: i32 = add(total, total);",
  '    io::println("done");',
  "}",
  "",
].join("\n");

function findBinary(repoRoot) {
  const candidates = [
    process.env.RAVUM_LSP_BIN,
    path.join(repoRoot, "target", "debug", "ravum-lang.exe"),
    path.join(repoRoot, "target", "debug", "ravum-lang"),
    path.join(repoRoot, "target", "release", "ravum-lang.exe"),
    path.join(repoRoot, "target", "release", "ravum-lang"),
  ].filter(Boolean);
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    "ravum-lang binary not found: run `cargo build -p ravum-lang-cli` first, " +
      "or set RAVUM_LSP_BIN. Looked at: " + candidates.join(", ")
  );
}

// An explicit executable is used when RAVUM_E2E_VSCODE is set (absolute path to
// a VS Code `Code.exe`). Otherwise @vscode/test-electron resolves and caches the
// official stable build, which is what CI and fresh machines use.
function findVSCode() {
  const explicit = process.env.RAVUM_E2E_VSCODE;
  if (explicit) {
    if (!fs.existsSync(explicit)) {
      throw new Error(`RAVUM_E2E_VSCODE points at a missing file: ${explicit}`);
    }
    return explicit;
  }
  return undefined;
}

function createWorkspace() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ravum-vscode-"));
  fs.mkdirSync(path.join(root, "src"), { recursive: true });
  fs.writeFileSync(
    path.join(root, "ravum.toml"),
    '[package]\nname = "vscodedemo"\nversion = "0.1.0"\nedition = "core-1.0"\n',
    "utf8"
  );
  fs.writeFileSync(path.join(root, "src", "main.rv"), SOURCE, "utf8");
  return root;
}

async function main() {
  // An inherited `ELECTRON_RUN_AS_NODE=1` turns every Electron binary (VS Code
  // included) into a plain Node process, so the window never opens and the
  // first argument is required as a module. The test runner must not leak it.
  delete process.env.ELECTRON_RUN_AS_NODE;

  const extensionDevelopmentPath = path.resolve(__dirname, "..");
  const extensionTestsPath = path.resolve(__dirname, "suite", "index.js");
  const repoRoot = path.resolve(extensionDevelopmentPath, "..", "..");

  const binary = findBinary(repoRoot);
  const workspace = createWorkspace();
  const vscodeExecutablePath = findVSCode();

  console.log(`[vscode-e2e] workspace   = ${workspace}`);
  console.log(`[vscode-e2e] server      = ${binary}`);
  console.log(`[vscode-e2e] vscode      = ${vscodeExecutablePath || "(downloaded by test-electron)"}`);

  try {
    await runTests({
      version: process.env.RAVUM_E2E_VSCODE_VERSION || "stable",
      vscodeExecutablePath,
      extensionDevelopmentPath,
      extensionTestsPath,
      extensionTestsEnv: {
        RAVUM_LSP_BIN: binary,
        RAVUM_E2E_WORKSPACE: workspace,
      },
      launchArgs: [
        workspace,
        "--disable-extensions",
        "--disable-workspace-trust",
        "--skip-welcome",
        "--skip-release-notes",
        "--no-sandbox",
      ],
    });
  } finally {
    if (!process.env.RAVUM_E2E_KEEP) {
      try {
        fs.rmSync(workspace, { recursive: true, force: true });
      } catch {
        // A leftover temp directory is not a test failure.
      }
    }
  }
}

main().catch((error) => {
  console.error("[vscode-e2e] FAILED");
  console.error(error && error.stack ? error.stack : error);
  process.exit(1);
});
