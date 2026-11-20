import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stat";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "12345");
const st = fs.statSync(p);
console.log("stat object:", st !== null && typeof st === "object");
console.log("stat isFile:", st.isFile());
console.log("stat isDirectory:", st.isDirectory());
console.log("stat size:", st.size);

console.log("stat uid number:", typeof st.uid === "number");
console.log("stat gid number:", typeof st.gid === "number");
