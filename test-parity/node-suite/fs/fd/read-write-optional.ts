import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fd_read_write_optional";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";

let fd = fs.openSync(p, "w+");
const buf = Buffer.from("hello world");
console.log("writeSync buffer slice bytes:", fs.writeSync(fd, buf, 0, 5, 0));
console.log("writeSync buffer pos bytes:", fs.writeSync(fd, buf, 6, 5, 6));
console.log("writeSync buffer content:", fs.readFileSync(p, "utf8"));

console.log("writeSync string pos bytes:", fs.writeSync(fd, "XX", 3, "utf8"));
console.log("writeSync string pos content:", fs.readFileSync(p, "utf8"));

const more = Buffer.from("!?!");
console.log("writeSync negative pos bytes:", fs.writeSync(fd, more, 0, 1, -1));
console.log("writeSync negative pos content:", fs.readFileSync(p, "utf8"));
fs.closeSync(fd);

fd = fs.openSync(p, "r");
const r1 = Buffer.alloc(5);
console.log("readSync pos bytes:", fs.readSync(fd, r1, 0, 5, 6));
console.log("readSync pos text:", r1.toString("utf8"));
const r2 = Buffer.alloc(2);
console.log("readSync current bytes:", fs.readSync(fd, r2, 0, 2, null));
console.log("readSync current text:", r2.toString("utf8"));
fs.closeSync(fd);
