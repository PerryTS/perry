import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_rename";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const a = ROOT + "/a.txt";
const b = ROOT + "/b.txt";
const c = ROOT + "/c.txt";
fs.writeFileSync(a, "move me");
fs.renameSync(a, b);
console.log("rename source gone:", !fs.existsSync(a));
console.log("rename dest content:", fs.readFileSync(b, "utf8"));
fs.copyFileSync(b, c);
console.log("copy content:", fs.readFileSync(c, "utf8"));
