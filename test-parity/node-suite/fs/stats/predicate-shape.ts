import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_shape";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.writeFileSync(ROOT + "/file.txt", "x");
const st = fs.statSync(ROOT + "/file.txt");
console.log("isFile typeof:", typeof st.isFile);
console.log("isDirectory typeof:", typeof st.isDirectory);
console.log("isSymbolicLink typeof:", typeof st.isSymbolicLink);
console.log("isSymbolicLink value:", st.isSymbolicLink());
