import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_sync_truncate";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";

let fh = await fsp.open(p, "w+");
console.log("fh datasync type:", typeof fh.datasync);
await fh.writeFile("hello world");
await fh.datasync();
await fh.truncate();
await fh.close();
console.log("fh truncate default size:", fs.statSync(p).size);

fh = await fsp.open(p, "w+");
await fh.writeFile("hi");
await fh.truncate(5);
await fh.close();
console.log("fh truncate extend size:", fs.statSync(p).size);
console.log("fh truncate extend prefix:", fs.readFileSync(p, "utf8").slice(0, 2));

fh = await fsp.open(p, "r+");
await fh.truncate(-1);
await fh.close();
console.log("fh truncate negative size:", fs.statSync(p).size);
