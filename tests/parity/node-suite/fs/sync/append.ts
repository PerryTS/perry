import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_append";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/append.txt";
fs.writeFileSync(p, "A");
fs.appendFileSync(p, "B");
fs.appendFileSync(p, "C");
console.log("append content:", fs.readFileSync(p, "utf8"));
