import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_utimes";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "time");

fs.utimesSync(p, 1_600_000_000, 1_600_000_123);
let stat = fs.statSync(p);
console.log("utimes mtime:", Math.round(stat.mtimeMs / 1000));
console.log("utimes atime number:", typeof stat.atimeMs);

const fd = fs.openSync(p, "r+");
fs.futimesSync(fd, 1_600_000_010, 1_600_000_456);
const fstat = fs.fstatSync(fd);
fs.closeSync(fd);
console.log("futimes mtime:", Math.round(fstat.mtimeMs / 1000));

const link = ROOT + "/link.txt";
fs.symlinkSync("file.txt", link);
fs.lutimesSync(link, 1_600_000_020, 1_600_000_789);
const lst = fs.lstatSync(link);
console.log("lutimes symlink:", lst.isSymbolicLink());
console.log("lutimes mtime:", Math.round(lst.mtimeMs / 1000));
