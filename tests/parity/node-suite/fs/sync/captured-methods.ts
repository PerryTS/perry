import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_captured";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const write = fs.writeFileSync;
const read = fs.readFileSync;
const exists = fs.existsSync;
write(ROOT + "/file.txt", "captured");
console.log("captured exists:", exists(ROOT + "/file.txt"));
console.log("captured read:", read(ROOT + "/file.txt", "utf8"));
