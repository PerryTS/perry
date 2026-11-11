import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_statfs_bigint_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const fields = ["type", "bsize", "blocks", "bfree", "bavail", "files", "ffree"];

function show(label: string, st: any) {
  console.log(`${label} types:`, fields.map((key) => typeof st[key]).join(","));
}

const st = await fsp.statfs(ROOT, { bigint: true });
show("promises statfs bigint", st);
