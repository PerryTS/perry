import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_readfile_position";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "Hello World");

const fh = await fsp.open(p, "r");
const buf = Buffer.alloc(5);
const rr = await fh.read(buf, 0, 5, null);
console.log("fh read bytes:", rr.bytesRead);
console.log("fh read head:", buf.toString("utf8"));
const rest = await fh.readFile();
console.log("fh readFile rest:", rest.toString("utf8"));
await fh.close();
