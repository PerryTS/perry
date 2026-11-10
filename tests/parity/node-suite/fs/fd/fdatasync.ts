import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fdatasync";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const fd = fs.openSync(p, "w+");
fs.writeSync(fd, "abc");
fs.fdatasyncSync(fd);
fs.fdatasync(fd, (err) => {
  console.log("fdatasync callback err:", err === null);
  fs.closeSync(fd);
  console.log("fdatasync content:", fs.readFileSync(p, "utf8"));
});
