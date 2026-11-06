import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_rw";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
console.log("exists before:", fs.existsSync(p));
fs.writeFileSync(p, "hello");
console.log("exists after:", fs.existsSync(p));
console.log("read utf8:", fs.readFileSync(p, "utf8"));
