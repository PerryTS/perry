import { WASI } from "node:wasi";
import { readFileSync } from "node:fs";
const module = new WebAssembly.Module(readFileSync("test-parity/node-suite/wasi/fixtures/exit-stops.wasm"));
const wasi = new WASI({ version: "preview1", returnOnExit: true });
const instance = new WebAssembly.Instance(module, wasi.getImportObject());
console.log("exit:", wasi.start(instance));
console.log("after exit:", new Uint8Array((instance.exports.memory as WebAssembly.Memory).buffer)[0]);
const replacement = new WebAssembly.Instance(module, { wasi_snapshot_preview1: { proc_exit(code: number) { console.log("override:", code); } } });
(replacement.exports._start as Function)();
console.log("after override:", new Uint8Array((replacement.exports.memory as WebAssembly.Memory).buffer)[0]);
let missing = false;
try { new WebAssembly.Instance(module, { wasi_snapshot_preview1: {} }); } catch { missing = true; }
console.log("missing import rejected:", missing);
