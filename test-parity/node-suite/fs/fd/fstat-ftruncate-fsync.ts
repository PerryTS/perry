import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fd_stats";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "abcdef");

const fd = fs.openSync(p, "r+");
const before = fs.fstatSync(fd);
console.log("fstat is file:", before.isFile());
console.log("fstat size before:", before.size);
fs.ftruncateSync(fd, 3);
fs.fsyncSync(fd);
const after = fs.fstatSync(fd);
fs.closeSync(fd);
console.log("fstat size after:", after.size);
console.log("ftruncate content:", fs.readFileSync(p, "utf8"));

const fd2 = fs.openSync(p, "r");
const fst = fs.fstatSync(fd2);
console.log("fstat uid number:", typeof fst.uid === "number");
console.log("fstat gid number:", typeof fst.gid === "number");
fs.closeSync(fd2);
