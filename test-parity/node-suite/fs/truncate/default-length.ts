import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_truncate_default_length";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const syncPath = ROOT + "/sync.txt";
fs.writeFileSync(syncPath, "abcdef");
fs.truncateSync(Buffer.from(syncPath));
console.log("truncateSync default size:", fs.statSync(syncPath).size);

const callbackPath = ROOT + "/callback.txt";
fs.writeFileSync(callbackPath, "abcdef");
fs.truncate(Buffer.from(callbackPath), (err) => {
  console.log("truncate callback default err:", err === null);
  console.log("truncate callback default size:", fs.statSync(callbackPath).size);
});
