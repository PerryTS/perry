import * as fs from "node:fs";

// readv/writev with empty buffers in the middle of the list should skip them
// without short-circuiting the surrounding reads/writes.
const ROOT = "/tmp/perry_node_suite_fs_readv_empty";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const path = ROOT + "/file.bin";
fs.writeFileSync(path, "ABCDEFGH");

const fd = fs.openSync(path, "r");
const a = Buffer.alloc(4);
const empty = Buffer.alloc(0);
const b = Buffer.alloc(4);
const total = fs.readvSync(fd, [a, empty, b], 0);
fs.closeSync(fd);
console.log("readv total:", total);
console.log("readv first:", a.toString("utf8"));
console.log("readv empty len:", empty.length);
console.log("readv last:", b.toString("utf8"));
