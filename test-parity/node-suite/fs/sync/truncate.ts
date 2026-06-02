import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_truncate";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "1234567890");
fs.truncateSync(p, 4);
console.log("truncate shorter:", fs.readFileSync(p, "utf8"));
fs.truncateSync(p, 6);
console.log("truncate extended size:", fs.statSync(p).size);
