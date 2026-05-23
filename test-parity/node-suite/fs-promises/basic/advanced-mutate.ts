import * as fsp from "node:fs/promises";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_promises_advanced";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);

const p = ROOT + "/file.txt";
await fsp.writeFile(p, "abcdef");
await fsp.truncate(p, 4);
console.log("truncate:", await fsp.readFile(p, "utf8"));

const copyDir = ROOT + "/copy";
await fsp.mkdir(ROOT + "/src");
await fsp.writeFile(ROOT + "/src/a.txt", "A");
await fsp.cp(ROOT + "/src", copyDir, { recursive: true });
console.log("cp recursive:", await fsp.readFile(copyDir + "/a.txt", "utf8"));

const link = ROOT + "/hard.txt";
await fsp.link(p, link);
console.log("hard link:", await fsp.readFile(link, "utf8"));

const sym = ROOT + "/sym.txt";
await fsp.symlink("file.txt", sym);
const target = await fsp.readlink(sym);
const lst = await fsp.lstat(sym);
console.log("readlink:", target);
console.log("lstat symlink:", lst.isSymbolicLink());

const tmp = await fsp.mkdtemp(ROOT + "/tmp-");
console.log("mkdtemp prefix:", tmp.indexOf(ROOT + "/tmp-") === 0);
await fsp.rmdir(tmp);
console.log("rmdir removed:", fs.existsSync(tmp));

await fsp.unlink(link);
console.log("unlink removed:", fs.existsSync(link));
