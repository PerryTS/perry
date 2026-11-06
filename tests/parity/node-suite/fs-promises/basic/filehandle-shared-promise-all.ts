import { open, mkdir, rm } from "node:fs/promises";

// FileHandle shared across concurrent awaits via Promise.all. Each read
// resolves with the bytes-read count and the buffer is mutated in place.
// We don't enforce a specific interleaving — both Node and Perry serialize
// fd reads — but the file pointer should advance monotonically and the
// reads together should cover the whole file.
const ROOT = "/tmp/perry_node_suite_fs_promises_fh_promise_all";
try { await rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await mkdir(ROOT, { recursive: true });

const path = ROOT + "/file.bin";
await (await open(path, "w")).writeFile("ABCDEFGHIJKLMNOP");

const fh = await open(path, "r");
const a = Buffer.alloc(4);
const b = Buffer.alloc(4);
const c = Buffer.alloc(4);
const results = await Promise.all([
  fh.read(a, 0, 4, null),
  fh.read(b, 0, 4, null),
  fh.read(c, 0, 4, null),
]);
await fh.close();

const sum = results.reduce((acc, r) => acc + r.bytesRead, 0);
console.log("filehandle Promise.all total bytes:", sum);
// Concatenate by buffer offset order — actual order can vary by impl.
const combined = Buffer.concat([a, b, c]).toString("utf8");
const sorted = combined.split("").sort().join("");
console.log("filehandle Promise.all union sorted:", sorted);
