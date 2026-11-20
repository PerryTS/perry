// #2133: `node:fs.promises` (the parent module's `.promises` namespace) was
// undefined under Perry, so test-fs-promises-file-handle-* tests of the form
// `const { open } = fs.promises` could not even reach FileHandle methods.
// This parity test exercises the parent-module shape end-to-end:
//   1. `typeof fs.promises === "object"` (both inline and via a binding).
//   2. destructured exports (`open`, `readFile`, `writeFile`, ...) are
//      callable functions.
//   3. a FileHandle returned by `fs.promises.open(...)` exposes the full
//      method surface and a write/stat/chmod/close round-trip succeeds.
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

console.log("typeof fs.promises:", typeof fs.promises);
const p: any = fs.promises;
console.log("typeof p:", typeof p);

const { open, readFile, writeFile, mkdir, rm } = fs.promises;
console.log("typeof open:", typeof open);
console.log("typeof readFile:", typeof readFile);
console.log("typeof writeFile:", typeof writeFile);
console.log("typeof mkdir:", typeof mkdir);
console.log("typeof rm:", typeof rm);

async function main() {
  const tmpDir = path.join(os.tmpdir(), "perry-2133-parent-promises");
  await rm(tmpDir, { recursive: true, force: true });
  await mkdir(tmpDir);
  const file = path.join(tmpDir, "f.txt");

  const fh = await open(file, "w+", 0o644);
  console.log("typeof fh.close:", typeof fh.close);
  console.log("typeof fh.read:", typeof fh.read);
  console.log("typeof fh.write:", typeof fh.write);
  console.log("typeof fh.chmod:", typeof fh.chmod);
  console.log("typeof fh.stat:", typeof fh.stat);
  console.log("typeof fh.readFile:", typeof fh.readFile);
  console.log("typeof fh.writeFile:", typeof fh.writeFile);
  console.log("typeof fh.appendFile:", typeof fh.appendFile);
  console.log("typeof fh.truncate:", typeof fh.truncate);
  console.log("typeof fh.utimes:", typeof fh.utimes);
  console.log("typeof fh.readv:", typeof fh.readv);
  console.log("typeof fh.writev:", typeof fh.writev);
  console.log("typeof fh.sync:", typeof fh.sync);

  await fh.writeFile("hello\n");
  const s = await fh.stat();
  console.log("stat.size:", s.size);
  await fh.chmod(0o600);
  await fh.close();

  await rm(tmpDir, { recursive: true, force: true });
  console.log("done");
}

main();
