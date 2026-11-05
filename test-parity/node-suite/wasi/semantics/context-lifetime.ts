import { WASI } from "node:wasi";
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
declare function gc(): void;
const root = mkdtempSync(join(tmpdir(), "perry-wasi-owner-"));
writeFileSync(join(root, "data.txt"), "retained");
const module = new WebAssembly.Module(readFileSync("test-parity/node-suite/wasi/fixtures/host-preview1.wasm"));
function makeExports(): any {
  const wasi = new WASI({ version: "preview1", preopens: { "/sandbox": root } });
  const instance = new WebAssembly.Instance(module, wasi.getImportObject());
  wasi.initialize(instance);
  return instance.exports;
}
const e = makeExports();
if (typeof gc === "function") gc();
console.log("retained open:", e.open(128, 8));
const fd = new DataView(e.memory.buffer).getUint32(64, true);
if (typeof gc === "function") gc();
console.log("retained read:", e.read(fd));
const size = new DataView(e.memory.buffer).getUint32(8, true);
console.log("retained value:", new TextDecoder().decode(new Uint8Array(e.memory.buffer, 512, size)));
console.log("retained close:", e.close(fd));
rmSync(root, { recursive: true, force: true });
let invalid = false;
try { invalid = e.bad_random() !== 0; } catch { invalid = true; }
console.log("invalid pointer rejected:", invalid);
console.log("host usable after rejection:", e.prestat() === 0);
