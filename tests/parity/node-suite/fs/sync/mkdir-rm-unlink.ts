import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_mkdir";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.mkdirSync(ROOT + "/child");
fs.writeFileSync(ROOT + "/child/a.txt", "x");
console.log("child exists:", fs.existsSync(ROOT + "/child"));
fs.unlinkSync(ROOT + "/child/a.txt");
console.log("file gone:", !fs.existsSync(ROOT + "/child/a.txt"));
fs.rmdirSync(ROOT + "/child");
console.log("dir gone:", !fs.existsSync(ROOT + "/child"));
