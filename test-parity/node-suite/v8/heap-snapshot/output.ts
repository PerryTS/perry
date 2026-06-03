// #3140: node:v8 heap snapshot output stream and file writer.
import * as v8 from "node:v8";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";

console.log(
  "exports:",
  typeof v8.getHeapSnapshot,
  v8.getHeapSnapshot.length,
  typeof v8.writeHeapSnapshot,
  v8.writeHeapSnapshot.length,
);

const stream: any = v8.getHeapSnapshot({ exposeInternals: true });
console.log(
  "stream shape:",
  typeof stream.read,
  typeof stream.on,
  typeof stream.pipe,
  typeof stream.setEncoding,
  typeof stream[Symbol.asyncIterator],
);
console.log("setEncoding returns this:", stream.setEncoding("utf8") === stream);
const [chunk] = await once(stream, "data");
console.log(
  "chunk:",
  typeof chunk,
  chunk.startsWith('{"snapshot"'),
  chunk.includes('"node_count"'),
);
stream.destroy?.();

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-v8-heap-"));
const file = path.join(dir, "explicit.heapsnapshot");
const ret = v8.writeHeapSnapshot(file, {});
const prefix = fs.readFileSync(ret, "utf8").slice(0, 40);
const stat = fs.statSync(ret);
console.log(
  "write:",
  ret === file,
  path.basename(ret),
  stat.size > 0,
  prefix.startsWith('{"snapshot"'),
  prefix.includes('"meta"'),
);
fs.rmSync(dir, { recursive: true, force: true });

for (const [label, call] of [
  ["get null options", () => v8.getHeapSnapshot(null as any)],
  ["get number options", () => v8.getHeapSnapshot(123 as any)],
  ["write bad path", () => v8.writeHeapSnapshot(123 as any)],
  [
    "write null options",
    () => v8.writeHeapSnapshot(path.join(os.tmpdir(), "perry-v8-bad.heapsnapshot"), null as any),
  ],
] as const) {
  try {
    call();
    console.log(label + ": no throw");
  } catch (e: any) {
    console.log(label + ":", e.name, e.code);
  }
}
