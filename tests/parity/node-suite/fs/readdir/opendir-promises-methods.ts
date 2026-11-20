import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_opendir_promises_methods";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/a.txt", "A");
fs.writeFileSync(ROOT + "/b.txt", "B");

const dir = fs.opendirSync(ROOT);
const first = await dir.read();
const second = await dir.read();
const done = await dir.read();
await dir.close();
const names = [first.name, second.name].sort();
console.log("fs Dir.read promise names:", names.join(","));
console.log("fs Dir.read promise end null:", done === null);
console.log("fs Dir.close promise:", true);
