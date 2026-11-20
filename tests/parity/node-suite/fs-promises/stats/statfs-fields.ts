import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_statfs_fields";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const fields = ["type", "bsize", "blocks", "bfree", "bavail", "files", "ffree"];

const stats = await fsp.statfs(ROOT);
console.log("promises statfs field types:", fields.map((key) => typeof (stats as any)[key]).join(","));
console.log(
  "promises statfs relationships:",
  stats.type >= 0 &&
    stats.bsize > 0 &&
    stats.blocks >= stats.bfree &&
    stats.bfree >= stats.bavail &&
    stats.files >= stats.ffree,
);
