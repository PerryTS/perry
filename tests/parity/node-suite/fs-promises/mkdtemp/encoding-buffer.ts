import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_mkdtemp_encoding_buffer";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const dir = await fsp.mkdtemp(ROOT + "/promises-", { encoding: "buffer" });
console.log("promises mkdtemp buffer encoding is buffer:", Buffer.isBuffer(dir));
console.log("promises mkdtemp buffer encoding exists:", fs.existsSync(dir));
