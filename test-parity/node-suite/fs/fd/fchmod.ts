import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fchmod";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "mode");
const fd = fs.openSync(p, "r+");
fs.fchmodSync(fd, 0o600);
console.log("fchmodSync mode:", (fs.fstatSync(fd).mode & 0o777).toString(8));
fs.fchmod(fd, 0o644, (err) => {
  console.log("fchmod callback err:", err === null);
  console.log("fchmod callback mode:", (fs.fstatSync(fd).mode & 0o777).toString(8));
  fs.closeSync(fd);
});
