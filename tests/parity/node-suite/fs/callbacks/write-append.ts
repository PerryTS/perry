import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cb_write";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
await new Promise<void>((resolve) => {
  fs.writeFile(p, "A", (err) => {
    console.log("write err null:", err === null);
    resolve();
  });
});
await new Promise<void>((resolve) => {
  fs.appendFile(p, "B", (err) => {
    console.log("append err null:", err === null);
    resolve();
  });
});
console.log("callback write content:", fs.readFileSync(p, "utf8"));
