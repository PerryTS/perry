import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_dirent";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.mkdirSync(ROOT + "/dir");
fs.writeFileSync(ROOT + "/file.txt", "f");
const entries = fs.readdirSync(ROOT, { withFileTypes: true }).slice().sort((a, b) => a.name.localeCompare(b.name));
console.log("dirent length:", entries.length);
console.log("first name:", entries[0].name);
console.log("first isDirectory:", entries[0].isDirectory());
console.log("second name:", entries[1].name);
console.log("second isFile:", entries[1].isFile());
