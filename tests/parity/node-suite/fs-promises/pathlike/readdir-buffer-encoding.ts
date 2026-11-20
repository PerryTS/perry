import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_readdir_buffer_encoding";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.writeFile(ROOT + "/a.txt", "a");
await fsp.writeFile(ROOT + "/b.txt", "b");

const entries = await fsp.readdir(Buffer.from(ROOT), { encoding: "buffer" });
console.log("promises entry isBuffer:", Buffer.isBuffer(entries[0]));
console.log("promises entries:", entries.map((x) => x.toString("utf8")).join(","));
