import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readdir";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.writeFileSync(ROOT + "/b.txt", "b");
fs.writeFileSync(ROOT + "/a.txt", "a");
const names = fs.readdirSync(ROOT).slice().sort();
console.log("readdir length:", names.length);
console.log("readdir first:", names[0]);
console.log("readdir second:", names[1]);
