import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fd_read_write_sync_object_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const path = ROOT + "/file.txt";
const fd = fs.openSync(path, "w+");
const buffer = Buffer.from("hello world!");

const written1 = (fs.writeSync as any)(fd, buffer, { offset: 0, length: 5, position: 0 });
console.log("writeSync object first bytes:", written1);
console.log("writeSync object first content:", fs.readFileSync(path, "utf8"));

const written2 = (fs.writeSync as any)(fd, buffer, { offset: 6, length: 2, position: 2 });
console.log("writeSync object second bytes:", written2);
console.log("writeSync object second content:", fs.readFileSync(path, "utf8"));

const current1 = fs.writeSync(fd, Buffer.from("!"), 0, 1, -1);
const current2 = fs.writeSync(fd, Buffer.from("?"), 0, 1, -1);
console.log("writeSync negative position bytes:", current1 + current2);
console.log("writeSync negative position content:", fs.readFileSync(path, "utf8"));

const readBuffer = Buffer.alloc(3);
const bytesRead = (fs.readSync as any)(fd, readBuffer, { offset: 0, length: 3, position: 1 });
console.log("readSync object bytes:", bytesRead);
console.log("readSync object text:", readBuffer.toString("utf8"));
fs.closeSync(fd);
