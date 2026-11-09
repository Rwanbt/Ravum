"use strict";

const path = require("path");
const Mocha = require("mocha");

function run() {
  const mocha = new Mocha({
    ui: "bdd",
    timeout: 180000,
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
