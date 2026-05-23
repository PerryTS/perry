import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_statfs_bigint_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const st = await fsp.statfs(ROOT, { bigint: true });
console.log("promises statfs bigint bsize type:", typeof st.bsize);
console.log("promises statfs bigint blocks type:", typeof st.blocks);
console.log("promises statfs bigint bfree type:", typeof st.bfree);
