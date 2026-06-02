import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_truncate_default_length";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const path = ROOT + "/file.txt";
await fsp.writeFile(path, "abcdef");
await fsp.truncate(Buffer.from(path));
console.log("promises truncate default size:", fs.statSync(path).size);
