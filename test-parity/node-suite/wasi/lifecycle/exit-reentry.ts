import { WASI } from "node:wasi";
import { readFileSync } from "node:fs";
const wasi = new WASI({ version: "preview1" });
const module = new WebAssembly.Module(readFileSync("test-parity/node-suite/wasi/fixtures/exit-reentry.wasm"));
let instance: any;
let afterCallback = false;
instance = new WebAssembly.Instance(module, {
  wasi_snapshot_preview1: wasi.wasiImport,
  env: { callback() { instance.exports.abort(); afterCallback = true; } },
});
console.log("nested exit:", wasi.start(instance));
console.log("after JS callback:", afterCallback);
console.log("after guest callback:", new Uint8Array(instance.exports.memory.buffer)[0]);
const direct = new WASI({ version: "preview1" });
let afterDirect = false;
console.log("direct exit:", direct.start({ exports: {
  memory: new WebAssembly.Memory({ initial: 1 }),
  _start() { direct.wasiImport.proc_exit(5); afterDirect = true; },
} } as any));
console.log("after direct:", afterDirect);

// Preserve the original JS exception across the interpreter boundary, then
// prove that the active-call frame and shared store remain usable.
const failing = new WASI({ version: "preview1" });
const sentinel = { message: "import failed" };
let shouldThrow = true;
const recoverable = new WebAssembly.Instance(module, {
  wasi_snapshot_preview1: failing.wasiImport,
  env: { callback() { if (shouldThrow) throw sentinel; } },
});
try { failing.start(recoverable); }
catch (error) { console.log("original exception:", error === sentinel); }
console.log("after thrown import:", new Uint8Array(recoverable.exports.memory.buffer)[0]);
shouldThrow = false;
recoverable.exports._start();
console.log("after recovery:", new Uint8Array(recoverable.exports.memory.buffer)[0]);
