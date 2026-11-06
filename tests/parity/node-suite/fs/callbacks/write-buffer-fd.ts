import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_write_buffer_fd";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const fd = fs.openSync(p, "w+");
const buf = Buffer.from("wXYZz");
fs.write(fd, buf, 1, 3, 0, (err, written, sameBuf) => {
  console.log("write buffer callback err:", err === null);
  console.log("write buffer callback bytes:", written);
  console.log("write buffer callback same:", sameBuf === buf);
  fs.closeSync(fd);
  console.log("write buffer callback text:", fs.readFileSync(p, "utf8"));
});
