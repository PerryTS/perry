import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_readfile_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "handle options");

const fh = await fsp.open(p, "r");
const text = await fh.readFile({ encoding: "utf8" });
console.log("fh object encoding:", text);

const buf = await fh.readFile({ encoding: null });
console.log("fh object null isBuffer:", Buffer.isBuffer(buf));
console.log("fh object null text:", buf.toString("utf8"));
await fh.close();
