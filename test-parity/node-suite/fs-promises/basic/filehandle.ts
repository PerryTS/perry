import * as fsp from "node:fs/promises";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";

const handle = await fsp.open(p, "w+");
console.log("filehandle fd number:", typeof handle.fd);
await handle.writeFile("abcdef");
await handle.sync();
const statBefore = await handle.stat();
console.log("filehandle stat size:", statBefore.size);
await handle.truncate(3);
await handle.close();
console.log("filehandle final content:", fs.readFileSync(p, "utf8"));

const readHandle = await fsp.open(p, "r");
const data = await readHandle.readFile("utf8");
await readHandle.close();
console.log("filehandle readFile:", data);

const fsStats = await fsp.statfs(ROOT);
console.log("promises statfs bsize positive:", fsStats.bsize > 0);
