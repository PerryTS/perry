import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_statfs_bigint_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const st = fs.statfsSync(ROOT, { bigint: true });
console.log("statfsSync bigint bsize type:", typeof st.bsize);
console.log("statfsSync bigint blocks type:", typeof st.blocks);
console.log("statfsSync bigint bfree type:", typeof st.bfree);

fs.statfs(ROOT, { bigint: true }, (err, cst) => {
  console.log("statfs callback bigint err:", err === null);
  console.log("statfs callback bigint bsize type:", typeof cst.bsize);
});
