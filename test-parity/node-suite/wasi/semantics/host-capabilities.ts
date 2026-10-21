import { WASI } from "node:wasi";
import { readFileSync, writeFileSync, mkdtempSync, mkdirSync, openSync, closeSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = mkdtempSync(join(tmpdir(), "perry-wasi-p1-"));
mkdirSync(join(root, "a"));
mkdirSync(join(root, "b"));
writeFileSync(join(root, "a", "data.txt"), "alpha");
writeFileSync(join(root, "b", "data.txt"), "beta");
writeFileSync(join(root, "outside.txt"), "secret");
symlinkSync(join(root, "outside.txt"), join(root, "a", "escape"));
const output = join(root, "stdout.txt");
const fd = openSync(output, "w");
const module = new WebAssembly.Module(readFileSync("test-parity/node-suite/wasi/fixtures/host-preview1.wasm"));
function create(path: string) {
  const wasi = new WASI({ version: "preview1", preopens: { "/sandbox": path }, stdout: fd });
  const instance = new WebAssembly.Instance(module, wasi.getImportObject());
  wasi.initialize(instance);
  return { wasi, e: instance.exports as any };
}
const a = create(join(root, "a"));
const b = create(join(root, "b"));
function word(e: any, offset: number) { return new DataView(e.memory.buffer).getUint32(offset, true); }
function contents(e: any) {
  const bytes = new Uint8Array(e.memory.buffer, 512, word(e, 8));
  return new TextDecoder().decode(bytes);
}
console.log("preopens:", a.e.prestat(), b.e.prestat());
console.log("open:", a.e.open(128, 8), b.e.open(128, 8));
const afd = word(a.e, 64), bfd = word(b.e, 64);
console.log("read a:", a.e.read(afd), contents(a.e));
console.log("close a:", a.e.close(afd));
console.log("read b:", b.e.read(bfd), contents(b.e));
console.log("parent denied:", a.e.open(160, 14) !== 0);
console.log("symlink denied:", a.e.open(192, 6) !== 0);
console.log("write:", a.e.write());
console.log("custom stdout:", readFileSync(output, "utf8"));
const previous = a.e.memory.buffer;
console.log("grown random:", a.e.grow_random());
console.log("grown length:", a.e.memory.buffer.byteLength);
console.log("old buffer detached:", previous.byteLength === 0);
console.log("random written:", new Uint8Array(a.e.memory.buffer, 65536, 32).some(v => v !== 0));
console.log("close guest stdout:", a.e.close(1));
try { closeSync(fd); } catch {} // Node closes the original; Perry owns a duplicate.
rmSync(root, { recursive: true, force: true });
