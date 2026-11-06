import { cp, mkdir, writeFile, rm, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_promises_cp_requires_recursive";
try { await rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await mkdir(ROOT, { recursive: true });
const src = ROOT + "/src";
const dest = ROOT + "/dest";
await mkdir(src);
await writeFile(src + "/file.txt", "promises-needs-recursive");

let rejected = false;
try {
  await cp(src, dest);
} catch (_e) {
  rejected = true;
}
console.log("promises.cp without recursive rejected:", rejected);
console.log("promises.cp dest absent:", !existsSync(dest));

await cp(src, dest, { recursive: true });
console.log("promises.cp with recursive file:", await readFile(dest + "/file.txt", "utf8"));
