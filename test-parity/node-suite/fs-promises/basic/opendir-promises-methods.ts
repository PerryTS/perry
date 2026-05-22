import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_opendir_methods";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.writeFile(ROOT + "/a.txt", "A");
await fsp.writeFile(ROOT + "/b.txt", "B");

const dir = await fsp.opendir(ROOT);
const first = await dir.read();
const second = await dir.read();
const done = await dir.read();
await dir.close();
const names = [first.name, second.name].sort();
console.log("promises dir.read names:", names.join(","));
console.log("promises dir.read end null:", done === null);
console.log("promises dir.close promise:", true);
