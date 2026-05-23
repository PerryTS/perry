import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_cp_advanced";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const src = ROOT + "/src";
const dest = ROOT + "/dest";
const derefDest = ROOT + "/deref";
fs.mkdirSync(src);
fs.writeFileSync(src + "/file.txt", "callback-file");
fs.symlinkSync(src + "/file.txt", src + "/link.txt");

await new Promise<void>((resolve) => {
  fs.cp(src, dest, { recursive: true }, (err) => {
    console.log("cp advanced callback err:", err === null);
    console.log("cp advanced callback file:", fs.readFileSync(dest + "/file.txt", "utf8"));
    console.log("cp advanced callback symlink:", fs.lstatSync(dest + "/link.txt").isSymbolicLink());
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.cp(src, derefDest, { recursive: true, dereference: true }, (err) => {
    console.log("cp advanced callback deref err:", err === null);
    console.log("cp advanced callback deref file:", fs.readFileSync(derefDest + "/link.txt", "utf8"));
    console.log("cp advanced callback deref lstat:", fs.lstatSync(derefDest + "/link.txt").isFile());
    resolve();
  });
});
