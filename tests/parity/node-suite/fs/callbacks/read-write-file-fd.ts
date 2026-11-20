import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_read_write_file_fd";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const fd = fs.openSync(p, "w+");

await new Promise<void>((resolve) => {
  fs.writeFile(fd, "fd-data", (err) => {
    console.log("writeFile fd err:", err === null);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.readFile(fd, "utf8", (err, data) => {
    console.log("readFile fd err:", err === null);
    console.log("readFile fd data:", data);
    resolve();
  });
});
fs.closeSync(fd);
