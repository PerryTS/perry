import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_statfs_fields";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const fields = ["type", "bsize", "blocks", "bfree", "bavail", "files", "ffree"];

function show(label: string, stats: any) {
  console.log(`${label} field types:`, fields.map((key) => typeof stats[key]).join(","));
  console.log(
    `${label} relationships:`,
    stats.type >= 0 &&
      stats.bsize > 0 &&
      stats.blocks >= stats.bfree &&
      stats.bfree >= stats.bavail &&
      stats.files >= stats.ffree,
  );
}

show("statfsSync", fs.statfsSync(ROOT));

fs.statfs(ROOT, (err, stats) => {
  console.log("statfs callback err:", err === null);
  show("statfs callback", stats);
});
