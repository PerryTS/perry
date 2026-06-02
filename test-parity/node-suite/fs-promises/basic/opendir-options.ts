import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_opendir_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.writeFile(ROOT + "/b.txt", "b");
await fsp.writeFile(ROOT + "/a.txt", "a");

const dir = await fsp.opendir(ROOT, { bufferSize: 1 });
console.log("promises opendir options path:", dir.path === ROOT);
const first = dir.readSync();
const second = await dir.read();
console.log("promises opendir options dirents:", [first.name + ":" + first.isFile(), second.name + ":" + second.isFile()].sort().join(","));
await dir.close();
console.log("promises opendir options closed:", true);
