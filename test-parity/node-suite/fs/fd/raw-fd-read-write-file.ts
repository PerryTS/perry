import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_raw_fd_file";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const fd = fs.openSync(p, "w+");
fs.writeFileSync(fd, "abc");
fs.appendFileSync(fd, Buffer.from("de"));
fs.closeSync(fd);
const rfd = fs.openSync(p, "r");
console.log("raw fd readFileSync:", fs.readFileSync(rfd, "utf8"));
fs.closeSync(rfd);
