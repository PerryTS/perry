import * as fs from "node:fs";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_write_copyfile_flags";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const copyFileSync = (fs as any)["copyFileSync"];

const appendPath = ROOT + "/append.txt";
fs.appendFileSync(appendPath, "A");
fs.appendFileSync(appendPath, "B", "utf8");
console.log("appendFileSync encoding:", fs.readFileSync(appendPath, "utf8"));

const src = ROOT + "/copy-src.txt";
const dst = ROOT + "/copy-dst.txt";
fs.writeFileSync(src, "source");
fs.writeFileSync(dst, "dest");
try { copyFileSync(src, dst, fs.constants.COPYFILE_EXCL); } catch (_e) {}
console.log("copyFileSync excl keeps existing:", fs.readFileSync(dst, "utf8"));
copyFileSync(src, dst);
console.log("copyFileSync overwrites:", fs.readFileSync(dst, "utf8"));
