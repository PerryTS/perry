import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cp_advanced";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const src = ROOT + "/src";
const dest = ROOT + "/dest";
const derefDest = ROOT + "/deref";
const preserveDest = ROOT + "/preserve";
fs.mkdirSync(src);
fs.mkdirSync(src + "/sub");
fs.writeFileSync(src + "/file.txt", "source-file");
fs.writeFileSync(src + "/sub/nested.txt", "nested-file");
fs.symlinkSync(src + "/file.txt", src + "/link.txt");

fs.cpSync(src, dest, { recursive: true });
console.log("cp advanced nested:", fs.readFileSync(dest + "/sub/nested.txt", "utf8"));
console.log("cp advanced symlink preserved:", fs.lstatSync(dest + "/link.txt").isSymbolicLink());
console.log("cp advanced symlink target suffix:", fs.readlinkSync(dest + "/link.txt").endsWith("file.txt"));

fs.cpSync(src, derefDest, { recursive: true, dereference: true });
console.log("cp advanced deref file:", fs.readFileSync(derefDest + "/link.txt", "utf8"));
console.log("cp advanced deref lstat file:", fs.lstatSync(derefDest + "/link.txt").isFile());

fs.writeFileSync(dest + "/file.txt", "existing");
fs.cpSync(src + "/file.txt", dest + "/file.txt", { force: false });
console.log("cp advanced force false:", fs.readFileSync(dest + "/file.txt", "utf8"));

const oldMs = Date.parse("2001-02-03T04:05:06.000Z");
const oldSeconds = oldMs / 1000;
fs.utimesSync(src + "/file.txt", oldSeconds, oldSeconds);
fs.cpSync(src + "/file.txt", preserveDest, { preserveTimestamps: true });
console.log("cp advanced preserve timestamp:", Math.abs(fs.statSync(preserveDest).mtimeMs - oldMs) < 2000);
