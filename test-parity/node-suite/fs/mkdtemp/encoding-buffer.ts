import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_mkdtemp_encoding_buffer";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const syncDir = fs.mkdtempSync(ROOT + "/sync-", { encoding: "buffer" });
console.log("mkdtempSync buffer encoding is buffer:", Buffer.isBuffer(syncDir));
console.log("mkdtempSync buffer encoding exists:", fs.existsSync(syncDir));

fs.mkdtemp(ROOT + "/callback-", { encoding: "buffer" }, (err, dir) => {
  console.log("mkdtemp callback buffer err:", err === null);
  console.log("mkdtemp callback buffer is buffer:", Buffer.isBuffer(dir));
  console.log("mkdtemp callback buffer exists:", fs.existsSync(dir));
});
