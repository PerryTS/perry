import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_writefile_fd_no_truncate";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const path = ROOT + "/file.txt";
fs.writeFileSync(path, "abcdef");

const syncFd = fs.openSync(path, "r+");
fs.writeFileSync(syncFd, "x");
fs.closeSync(syncFd);
console.log("writeFileSync fd no truncate:", fs.readFileSync(path, "utf8"));

fs.writeFileSync(path, "abcdef");
const cbFd = fs.openSync(path, "r+");
fs.writeFile(cbFd, "y", (err) => {
  fs.closeSync(cbFd);
  console.log("writeFile callback fd err:", err === null);
  console.log("writeFile callback fd no truncate:", fs.readFileSync(path, "utf8"));
});
