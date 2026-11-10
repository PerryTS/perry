import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_open_close";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "open-close");

fs.open(p, "r", (err, fd) => {
  console.log("open callback err:", err === null);
  console.log("open callback fd number:", typeof fd);
  const buf = Buffer.alloc(4);
  fs.read(fd, buf, 0, 4, 0, (readErr, bytes) => {
    console.log("open callback read err:", readErr === null);
    console.log("open callback read bytes:", bytes);
    console.log("open callback read text:", buf.toString("utf8"));
    fs.close(fd, (closeErr) => {
      console.log("close callback err:", closeErr === null);
    });
  });
});
