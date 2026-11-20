import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_readv_writev";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const fd = fs.openSync(p, "w+");

await new Promise<void>((resolve) => {
  fs.writev(fd, [Buffer.from("ab"), Buffer.from("cd")], 0, (err, bytesWritten, buffers) => {
    console.log("writev callback err:", err === null);
    console.log("writev callback bytes:", bytesWritten);
    console.log("writev callback buffers:", buffers.length);
    resolve();
  });
});

const b1 = Buffer.alloc(1);
const b2 = Buffer.alloc(2);
await new Promise<void>((resolve) => {
  fs.readv(fd, [b1, b2], 1, (err, bytesRead, buffers) => {
    console.log("readv callback err:", err === null);
    console.log("readv callback bytes:", bytesRead);
    console.log("readv callback buffers:", buffers.length);
    console.log("readv callback text:", b1.toString("utf8") + b2.toString("utf8"));
    resolve();
  });
});
fs.closeSync(fd);
