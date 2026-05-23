import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_mkdtemp_buffer_prefix";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const made = await fsp.mkdtemp(Buffer.from(ROOT + "/tmp-"));
console.log("promises prefix:", made.indexOf(ROOT + "/tmp-") === 0);
console.log("promises exists:", (await fsp.stat(made)).isDirectory());
