import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stat_dir";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const st = fs.statSync(ROOT);
console.log("dir isFile:", st.isFile());
console.log("dir isDirectory:", st.isDirectory());
console.log("mode number:", typeof st.mode === "number");
