import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_read_write";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";

const wh = await fsp.open(p, "w+");
const wr = await wh.write("abcdef");
console.log("fh write bytes:", wr.bytesWritten);
console.log("fh write buffer:", wr.buffer);
await wh.close();

const rh = await fsp.open(p, "r");
const buf = Buffer.alloc(4);
const rr = await rh.read(buf, 0, 4, 2);
console.log("fh read bytes:", rr.bytesRead);
console.log("fh read same buffer:", rr.buffer === buf);
console.log("fh read text:", buf.toString("utf8"));
await rh.close();
