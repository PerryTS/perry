import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_links";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const target = ROOT + "/target.txt";
const hard = ROOT + "/hard.txt";
const sym = ROOT + "/sym.txt";
fs.writeFileSync(target, "linked");
fs.linkSync(target, hard);
console.log("hardlink content:", fs.readFileSync(hard, "utf8"));
fs.symlinkSync(target, sym);
console.log("readlink target:", fs.readlinkSync(sym));
console.log("lstat isSymbolicLink:", fs.lstatSync(sym).isSymbolicLink());
console.log("stat follows link:", fs.statSync(sym).isFile());
