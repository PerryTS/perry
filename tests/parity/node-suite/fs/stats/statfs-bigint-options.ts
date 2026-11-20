import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_statfs_bigint_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const fields = ["type", "bsize", "blocks", "bfree", "bavail", "files", "ffree"];

function show(label: string, st: any) {
  console.log(`${label} types:`, fields.map((key) => typeof st[key]).join(","));
}

const st = fs.statfsSync(ROOT, { bigint: true });
show("statfsSync bigint", st);

fs.statfs(ROOT, { bigint: true }, (err, cst) => {
  console.log("statfs callback bigint err:", err === null);
  show("statfs callback bigint", cst);
});
