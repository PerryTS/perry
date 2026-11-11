import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_links";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const target = ROOT + "/target.txt";
const hard = ROOT + "/hard.txt";
const sym = ROOT + "/sym.txt";
await fsp.writeFile(target, "linked");
await fsp.link(target, hard);
console.log("promises hardlink content:", await fsp.readFile(hard, "utf8"));
console.log("promises hardlink nlink:", (await fsp.stat(target)).nlink >= 2);
await fsp.symlink(target, sym);
const linkTarget = await fsp.readlink(sym);
console.log("promises readlink target suffix:", linkTarget.endsWith("target.txt"));
console.log("promises lstat symlink:", (await fsp.lstat(sym)).isSymbolicLink());
console.log("promises stat follows link:", (await fsp.stat(sym)).isFile());
