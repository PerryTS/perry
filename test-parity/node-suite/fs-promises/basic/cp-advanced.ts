import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_cp_advanced";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const src = ROOT + "/src";
const dest = ROOT + "/dest";
const derefDest = ROOT + "/deref";
const preserveDest = ROOT + "/preserve.txt";
await fsp.mkdir(src);
await fsp.mkdir(src + "/sub");
await fsp.writeFile(src + "/file.txt", "promises-file");
await fsp.writeFile(src + "/sub/nested.txt", "promises-nested");
await fsp.symlink(src + "/file.txt", src + "/link.txt");

await fsp.cp(src, dest, { recursive: true });
console.log("promises cp advanced nested:", await fsp.readFile(dest + "/sub/nested.txt", "utf8"));
console.log("promises cp advanced symlink:", (await fsp.lstat(dest + "/link.txt")).isSymbolicLink());
console.log("promises cp advanced target:", (await fsp.readlink(dest + "/link.txt")).endsWith("file.txt"));

await fsp.cp(src, derefDest, { recursive: true, dereference: true });
console.log("promises cp advanced deref:", await fsp.readFile(derefDest + "/link.txt", "utf8"));
console.log("promises cp advanced deref lstat:", (await fsp.lstat(derefDest + "/link.txt")).isFile());

await fsp.writeFile(dest + "/file.txt", "existing");
await fsp.cp(src + "/file.txt", dest + "/file.txt", { force: false });
console.log("promises cp advanced force false:", await fsp.readFile(dest + "/file.txt", "utf8"));

const oldMs = Date.parse("2002-03-04T05:06:07.000Z");
const oldSeconds = oldMs / 1000;
fs.utimesSync(src + "/file.txt", oldSeconds, oldSeconds);
await fsp.cp(src + "/file.txt", preserveDest, { preserveTimestamps: true });
console.log("promises cp advanced preserve timestamp:", Math.abs((await fsp.stat(preserveDest)).mtimeMs - oldMs) < 2000);
