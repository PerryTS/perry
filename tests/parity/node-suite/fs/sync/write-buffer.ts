import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_write_buffer";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.bin";
fs.writeFileSync(p, Buffer.from([0x61, 0x62, 0x63]));
fs.appendFileSync(p, Buffer.from([0x64, 0x65]));
console.log("write buffer text:", fs.readFileSync(p, "utf8"));
const fd = fs.openSync(ROOT + "/fd.bin", "w");
console.log("writeSync buffer bytes:", fs.writeSync(fd, Buffer.from("XYZ"), 1, 2, 0));
fs.closeSync(fd);
console.log("writeSync buffer text:", fs.readFileSync(ROOT + "/fd.bin", "utf8"));
