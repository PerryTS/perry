import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_dirent_predicates";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.mkdirSync(ROOT + "/dir");
fs.writeFileSync(ROOT + "/file.txt", "f");
const entries = fs.readdirSync(ROOT, { withFileTypes: true }).slice().sort((a, b) => a.name.localeCompare(b.name));
console.log("dir isFile:", entries[0].isFile());
console.log("dir isDirectory:", entries[0].isDirectory());
console.log("dir isSymbolicLink:", entries[0].isSymbolicLink());
console.log("file isFile:", entries[1].isFile());
console.log("file isDirectory:", entries[1].isDirectory());
