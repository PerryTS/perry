import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cb_read";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.writeFileSync(ROOT + "/file.txt", "callback");
await new Promise<void>((resolve) => {
  fs.readFile(ROOT + "/file.txt", "utf8", (err, data) => {
    console.log("callback err null:", err === null);
    console.log("callback data:", data);
    resolve();
  });
});
