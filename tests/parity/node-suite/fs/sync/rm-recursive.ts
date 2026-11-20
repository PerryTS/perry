import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_rm_recursive";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.mkdirSync(ROOT + "/child");
fs.writeFileSync(ROOT + "/child/file.txt", "x");
console.log("nested exists before:", fs.existsSync(ROOT + "/child/file.txt"));
fs.rmSync(ROOT, { recursive: true, force: true });
console.log("root gone after:", !fs.existsSync(ROOT));
