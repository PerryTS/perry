import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_basic";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "hello");
console.log("read utf8:", await fsp.readFile(p, "utf8"));
const buf = await fsp.readFile(p);
console.log("read buffer isBuffer:", Buffer.isBuffer(buf));
console.log("read buffer text:", buf.toString("utf8"));
await fsp.appendFile(p, " world");
console.log("append content:", await fsp.readFile(p, "utf8"));
