import * as fs from "node:fs";

// COPYFILE_EXCL must fail when the destination already exists.
const ROOT = "/tmp/perry_node_suite_fs_copyfile_excl";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const src = ROOT + "/src.txt";
const dest = ROOT + "/dest.txt";
fs.writeFileSync(src, "exclusive-src");
fs.writeFileSync(dest, "preexisting-dest");

let threw = false;
try {
  fs.copyFileSync(src, dest, fs.constants.COPYFILE_EXCL);
} catch (_e) {
  threw = true;
}
console.log("copyFile EXCL on existing dest threw:", threw);
console.log("copyFile EXCL preserved dest:", fs.readFileSync(dest, "utf8"));

// Sanity: without EXCL the copy overwrites.
fs.copyFileSync(src, dest);
console.log("copyFile without EXCL overwrites:", fs.readFileSync(dest, "utf8"));
