import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_chmod";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "x");
fs.chmodSync(p, 0o644);
const st = fs.statSync(p);
console.log("mode number:", typeof st.mode === "number");
console.log("mode suffix:", (st.mode & 0o777).toString(8));
