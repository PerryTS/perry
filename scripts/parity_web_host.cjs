// Runs the page `perry --target web` emits under Node, so `scripts/parity.py --mode web`
// can compare its console output with Node's run of the same source.
//
//     node scripts/parity_web_host.cjs PAGE.html [ARG ...]
//
// The page carries the runtime in its first <script> and the module as base64 in
// `window.__perryWasmB64`. An uncaught error prints only its stack and exits 1: the
// source line Node prints above it would name this host's eval, not the case.
"use strict";
const fs = require("fs");

const html = fs.readFileSync(process.argv[2], "utf8");
const runtime = /<script>([\s\S]*?)<\/script>/.exec(html);
const wasm = /window\.__perryWasmB64 = "([^"]*)"/.exec(html);
if (!runtime || !wasm) {
  console.error(`parity_web_host: ${process.argv[2]} is not a perry --target web page`);
  process.exit(2);
}
process.argv.splice(1, 2, process.argv[2]);

const element = () => ({
  id: "", style: {}, textContent: "", children: [],
  appendChild(child) { this.children.push(child); return child; },
  removeChild() {}, setAttribute() {}, addEventListener() {},
});
globalThis.window = globalThis;
globalThis.document = {
  title: "", head: element(), body: element(),
  createElement: element, createTextNode: element,
  getElementById: () => null, querySelector: () => null, addEventListener() {},
};
globalThis.requestAnimationFrame = (cb) => setTimeout(() => cb(performance.now()), 0);
globalThis.cancelAnimationFrame = (id) => clearTimeout(id);

const uncaught = (error) => {
  console.error((error && error.stack) || String(error));
  process.exit(1);
};
process.on("uncaughtException", uncaught);
process.on("unhandledRejection", uncaught);
// Indirect eval runs the runtime at global scope; its completion value is the boot function.
const boot = (0, eval)(`${runtime[1]}\n;bootPerryWasm`);
boot(wasm[1]).catch(uncaught);
