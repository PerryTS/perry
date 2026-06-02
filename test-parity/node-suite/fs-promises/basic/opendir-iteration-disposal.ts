import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_opendir_iteration_disposal";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.writeFile(ROOT + "/a.txt", "A");
await fsp.writeFile(ROOT + "/b.txt", "B");

async function closedRead(label: string, dir: fs.Dir) {
  try {
    await dir.read();
    console.log(label, "open");
  } catch (e: any) {
    console.log(label, e.code);
  }
}

const dir = await fsp.opendir(ROOT);
console.log(
  "promises dir iteration surface:",
  typeof (dir as any).entries,
  typeof (dir as any)[Symbol.asyncIterator],
  typeof (dir as any)[Symbol.dispose],
  typeof (dir as any)[Symbol.asyncDispose],
);
const entryNames: string[] = [];
for await (const entry of (dir as any).entries()) {
  entryNames.push(entry.name + ":" + entry.isFile());
}
console.log("promises dir entries names:", entryNames.sort().join(","));
await closedRead("promises dir entries closed:", dir);

const directDir = await fsp.opendir(ROOT);
const directNames: string[] = [];
for await (const entry of (directDir as any)) {
  directNames.push(entry.name);
}
console.log("promises dir direct async names:", directNames.sort().join(","));
await closedRead("promises dir direct closed:", directDir);

const disposedDir = await fsp.opendir(ROOT);
(disposedDir as any)[Symbol.dispose]();
await closedRead("promises dir dispose closed:", disposedDir);
console.log("promises dir dispose idempotent:", (disposedDir as any)[Symbol.dispose]() === undefined);

const asyncDisposedDir = await fsp.opendir(ROOT);
console.log("promises dir asyncDispose ret:", (await (asyncDisposedDir as any)[Symbol.asyncDispose]()) === undefined);
await closedRead("promises dir asyncDispose closed:", asyncDisposedDir);
console.log("promises dir asyncDispose idempotent:", (await (asyncDisposedDir as any)[Symbol.asyncDispose]()) === undefined);

try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
