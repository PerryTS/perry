import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fd";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "abcdef");
const fd = fs.openSync(p, "r");
const buf = Buffer.alloc(4);
const read = fs.readSync(fd, buf, 0, 4, 1);
fs.closeSync(fd);
console.log("readSync bytes:", read);
console.log("readSync text:", buf.toString("utf8"));
const out = ROOT + "/out.txt";
const wfd = fs.openSync(out, "w");
const written = fs.writeSync(wfd, "hello");
fs.closeSync(wfd);
console.log("writeSync bytes:", written);
console.log("writeSync content:", fs.readFileSync(out, "utf8"));
