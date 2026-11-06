import * as fs from "node:fs";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_cp_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const source = ROOT + "/source.txt";
const dest = ROOT + "/dest.txt";
fs.writeFileSync(source, "new");
fs.writeFileSync(dest, "old");
fs.cpSync(source, dest, { force: false });
console.log("cp force false keeps existing:", fs.readFileSync(dest, "utf8"));
fs.cpSync(source, dest, { force: true });
console.log("cp force true overwrites:", fs.readFileSync(dest, "utf8"));

fs.utimesSync(source, 981173106, 1015218367);
const preserve = ROOT + "/preserve.txt";
fs.cpSync(source, preserve, { preserveTimestamps: true });
const preserved = fs.statSync(preserve);
console.log("cp preserve timestamp seconds:", Math.floor(preserved.mtimeMs / 1000));

const tree = ROOT + "/tree";
fs.mkdirSync(tree + "/nested", { recursive: true });
fs.writeFileSync(tree + "/nested/file.txt", "tree");
fs.cpSync(tree, ROOT + "/tree-copy", { recursive: true, force: false });
console.log("cp force false tree:", fs.readFileSync(ROOT + "/tree-copy/nested/file.txt", "utf8"));

const target = ROOT + "/target.txt";
const link = ROOT + "/link.txt";
fs.writeFileSync(target, "target-data");
fs.symlinkSync("target.txt", link);
fs.cpSync(link, ROOT + "/link-deref.txt", { dereference: true, recursive: true });
console.log("cp dereference symlink:", fs.readFileSync(ROOT + "/link-deref.txt", "utf8"));
