import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readv_writev";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const fd = fs.openSync(p, "w+");
const written = fs.writevSync(fd, [Buffer.from("ab"), Buffer.from("cd"), Buffer.from("ef")]);
console.log("writev bytes:", written);
fs.fsyncSync(fd);
const b1 = Buffer.alloc(2);
const b2 = Buffer.alloc(3);
const read = fs.readvSync(fd, [b1, b2], 1);
fs.closeSync(fd);
console.log("readv bytes:", read);
console.log("readv text:", b1.toString("utf8") + ":" + b2.toString("utf8"));
console.log("writev content:", fs.readFileSync(p, "utf8"));
