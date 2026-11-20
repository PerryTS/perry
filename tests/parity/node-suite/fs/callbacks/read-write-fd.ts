import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_read_write_fd";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const fd = fs.openSync(p, "w+");

fs.write(fd, "abcdef", (err, written, value) => {
  console.log("write callback err:", err === null);
  console.log("write callback bytes:", written);
  console.log("write callback value:", value);
  const buf = Buffer.alloc(4);
  fs.read(fd, buf, 0, 4, 2, (err2, read, sameBuf) => {
    console.log("read callback err:", err2 === null);
    console.log("read callback bytes:", read);
    console.log("read callback same buffer:", sameBuf === buf);
    console.log("read callback text:", buf.toString("utf8"));
    fs.closeSync(fd);
  });
});
