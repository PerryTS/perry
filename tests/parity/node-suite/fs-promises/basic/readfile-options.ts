import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_readfile_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "promise options");

const text = await fsp.readFile(p, { encoding: "utf8", flag: "r" });
console.log("object encoding:", text);

const buf = await fsp.readFile(p, { encoding: null, flag: "r" });
console.log("object null isBuffer:", Buffer.isBuffer(buf));
console.log("object null text:", buf.toString("utf8"));

const flagOnly = await fsp.readFile(p, { flag: "r" });
console.log("flag only isBuffer:", Buffer.isBuffer(flagOnly));
console.log("flag only text:", flagOnly.toString("utf8"));
