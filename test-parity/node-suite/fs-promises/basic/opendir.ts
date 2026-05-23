import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_opendir";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
await fsp.writeFile(ROOT + "/a.txt", "A");
await fsp.writeFile(ROOT + "/b.txt", "B");

const dir = await fsp.opendir(ROOT);
console.log("promises dir path:", dir.path === ROOT);
const a = dir.readSync();
const b = dir.readSync();
const end = dir.readSync();
const names = [a.name, b.name].sort();
console.log("promises opendir names:", names.join(","));
console.log("promises opendir end:", end === null);
await new Promise((resolve) => dir.close(resolve));
