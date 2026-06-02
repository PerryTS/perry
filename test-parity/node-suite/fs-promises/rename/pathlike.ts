import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_rename_pathlike";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const a = ROOT + "/a [] á.txt";
const b = ROOT + "/b [] á.txt";
await fsp.writeFile(a, "promises");
await fsp.rename(Buffer.from(a), Buffer.from(b));
console.log("promises rename pathlike source gone:", !fs.existsSync(a));
console.log("promises rename pathlike dest content:", await fsp.readFile(b, "utf8"));
