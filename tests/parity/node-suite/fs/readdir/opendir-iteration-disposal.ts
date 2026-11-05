import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_opendir_iteration_disposal";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/a.txt", "A");
fs.writeFileSync(ROOT + "/b.txt", "B");

function closedReadSync(label: string, dir: fs.Dir) {
  try {
    dir.readSync();
    console.log(label, "open");
  } catch (e: any) {
    console.log(label, e.code);
  }
}

async function closedRead(label: string, dir: fs.Dir) {
  try {
    await dir.read();
    console.log(label, "open");
  } catch (e: any) {
    console.log(label, e.code);
  }
}

const dir = fs.opendirSync(ROOT);
console.log(
  "dir iteration surface:",
  typeof (dir as any).entries,
  typeof (dir as any)[Symbol.asyncIterator],
  typeof (dir as any)[Symbol.dispose],
  typeof (dir as any)[Symbol.asyncDispose],
);
const entryNames: string[] = [];
for await (const entry of (dir as any).entries()) {
  entryNames.push(entry.name + ":" + entry.isFile());
}
console.log("dir entries names:", entryNames.sort().join(","));
closedReadSync("dir entries closed:", dir);

const directDir = fs.opendirSync(ROOT);
const directNames: string[] = [];
for await (const entry of (directDir as any)) {
  directNames.push(entry.name);
}
console.log("dir direct async names:", directNames.sort().join(","));
await closedRead("dir direct closed:", directDir);

const disposedDir = fs.opendirSync(ROOT);
(disposedDir as any)[Symbol.dispose]();
closedReadSync("dir dispose closed:", disposedDir);
console.log("dir dispose idempotent:", (disposedDir as any)[Symbol.dispose]() === undefined);

const asyncDisposedDir = fs.opendirSync(ROOT);
console.log("dir asyncDispose ret:", (await (asyncDisposedDir as any)[Symbol.asyncDispose]()) === undefined);
await closedRead("dir asyncDispose closed:", asyncDisposedDir);
console.log("dir asyncDispose idempotent:", (await (asyncDisposedDir as any)[Symbol.asyncDispose]()) === undefined);

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
