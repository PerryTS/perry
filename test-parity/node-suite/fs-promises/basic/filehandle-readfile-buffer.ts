import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_readfile_buffer";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "buffer-data");

const fh = await fsp.open(p, "r");
const data = await fh.readFile();
console.log("fh readFile buffer text:", data.toString("utf8"));
await fh.close();
