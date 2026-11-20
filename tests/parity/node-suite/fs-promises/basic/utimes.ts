import * as fsp from "node:fs/promises";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_promises_utimes";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "time");
await fsp.utimes(p, 1_600_000_555, 1_600_000_666);
let stat = await fsp.stat(p);
console.log("promises utimes mtime:", Math.round(stat.mtimeMs / 1000));

const handle = await fsp.open(p, "r+");
await handle.utimes(1_600_000_777, 1_600_000_888);
const hstat = await handle.stat();
await handle.close();
console.log("filehandle utimes mtime:", Math.round(hstat.mtimeMs / 1000));

const link = ROOT + "/link.txt";
fs.symlinkSync("file.txt", link);
await fsp.lutimes(link, 1_600_000_999, 1_600_001_000);
const lst = await fsp.lstat(link);
console.log("promises lutimes symlink:", lst.isSymbolicLink());
console.log("promises lutimes mtime:", Math.round(lst.mtimeMs / 1000));
