import * as fs from "node:fs";

// Node rejects `cpSync(dir, dest)` without `recursive: true` with ERR_FS_EISDIR.
// Perry surfaces this as a thrown error too — verify the call fails and the
// dest is not created.
const ROOT = "/tmp/perry_node_suite_fs_cp_requires_recursive";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const src = ROOT + "/src";
const dest = ROOT + "/dest";
fs.mkdirSync(src);
fs.writeFileSync(src + "/file.txt", "needs-recursive");

let threw = false;
try {
  fs.cpSync(src, dest);
} catch (_e) {
  threw = true;
}
console.log("cp without recursive threw:", threw);
console.log("cp without recursive dest absent:", !fs.existsSync(dest));

// Sanity: with `recursive: true` it works.
fs.cpSync(src, dest, { recursive: true });
console.log("cp with recursive file:", fs.readFileSync(dest + "/file.txt", "utf8"));
