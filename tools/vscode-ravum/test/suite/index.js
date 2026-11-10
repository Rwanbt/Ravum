"use strict";

const path = require("path");
const Mocha = require("mocha");

function run() {
  const mocha = new Mocha({
    ui: "bdd",
    // The extension host starts a real `ravum-lang lsp` child process. Under a
    // loaded machine (concurrent cargo builds, CI runners) activation and the
    // first LSP round-trip can take far longer than the old 3-minute budget,
    // which used to fail the suite as a timeout rather than a real defect.
    timeout: 300000,
    color: true,
  });
  mocha.addFile(path.join(__dirname, "lsp.test.js"));

  return new Promise((resolve, reject) => {
    mocha.run((failures) => {
      if (failures > 0) {
        reject(new Error(`${failures} VS Code integration test(s) failed`));
      } else {
        resolve();
      }
    });
  });
}

module.exports = { run };
