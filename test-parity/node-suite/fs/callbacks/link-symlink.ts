import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callbacks_link_symlink";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const target = ROOT + "/target.txt";
const hard = ROOT + "/hard.txt";
const sym = ROOT + "/sym.txt";
const typedSym = ROOT + "/typed-sym.txt";
fs.writeFileSync(target, "linked-callback");

await new Promise<void>((resolve) => {
  fs.link(target, hard, (err) => {
    console.log("link callback err:", err === null);
    console.log("link callback content:", fs.readFileSync(hard, "utf8"));
    console.log("link callback nlink:", fs.statSync(target).nlink >= 2);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.symlink(target, sym, (err) => {
    console.log("symlink callback err:", err === null);
    console.log("symlink callback lstat:", fs.lstatSync(sym).isSymbolicLink());
    console.log("symlink callback readlink:", fs.readlinkSync(sym).endsWith("target.txt"));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.symlink(target, typedSym, "file", (err) => {
    console.log("symlink typed callback err:", err === null);
    console.log("symlink typed callback stat:", fs.statSync(typedSym).isFile());
    resolve();
  });
});
