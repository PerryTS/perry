import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_vector_append";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";

const handle = await fsp.open(p, "w+");
const wr = await handle.writev([Buffer.from("ab"), Buffer.from("cd")]);
console.log("fh writev bytes:", wr.bytesWritten);
console.log("fh writev same buffers:", Array.isArray(wr.buffers));
await handle.appendFile("ef");
await handle.sync();
const b1 = Buffer.alloc(2);
const b2 = Buffer.alloc(3);
const rr = await handle.readv([b1, b2], 1);
console.log("fh readv bytes:", rr.bytesRead);
console.log("fh readv same buffers:", Array.isArray(rr.buffers));
console.log("fh readv text:", b1.toString("utf8") + ":" + b2.toString("utf8"));
await handle.close();
console.log("fh append content:", await fsp.readFile(p, "utf8"));
