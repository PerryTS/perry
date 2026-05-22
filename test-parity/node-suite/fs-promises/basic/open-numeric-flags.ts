import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_open_numeric_flags";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const c = fs.constants;

const created = ROOT + "/created.txt";
let fh = await fsp.open(created, c.O_CREAT | c.O_WRONLY | c.O_TRUNC);
await fh.writeFile("numeric");
await fh.close();
console.log("promises open numeric create write:", await fsp.readFile(created, "utf8"));

fh = await fsp.open(created, c.O_RDWR | c.O_APPEND);
await fh.writeFile("-append");
await fh.close();
console.log("promises open numeric append:", await fsp.readFile(created, "utf8"));

const exclusive = ROOT + "/exclusive.txt";
fh = await fsp.open(exclusive, c.O_CREAT | c.O_WRONLY | c.O_EXCL);
await fh.writeFile("exclusive");
await fh.close();
console.log("promises open numeric exclusive content:", await fsp.readFile(exclusive, "utf8"));
let exclusiveFailed = false;
try { const existing = await fsp.open(exclusive, c.O_CREAT | c.O_WRONLY | c.O_EXCL); exclusiveFailed = existing.fd < 0; await existing.close(); } catch (_e) { exclusiveFailed = true; }
console.log("promises open numeric exclusive existing failed:", exclusiveFailed);
