import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_chmod";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "mode");
const handle = await fsp.open(p, "r+");
await handle.chmod(0o600);
let stats = await handle.stat();
console.log("filehandle chmod mode:", (stats.mode & 0o777).toString(8));
await handle.close();
