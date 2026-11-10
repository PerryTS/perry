// fs.statSync / lstatSync with { throwIfNoEntry: false } return undefined
// when the path does not exist; every other case still throws.
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_noentry";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.writeFileSync(ROOT + "/file.txt", "x");
const missing = ROOT + "/missing";

console.log("stat missing:", fs.statSync(missing, { throwIfNoEntry: false }));
console.log("lstat missing:", fs.lstatSync(missing, { throwIfNoEntry: false }));
console.log("stat missing bigint:", fs.statSync(missing, { throwIfNoEntry: false, bigint: true }));
console.log("stat under file:", fs.statSync(ROOT + "/file.txt/x", { throwIfNoEntry: false }));
const present = fs.statSync(ROOT + "/file.txt", { throwIfNoEntry: false });
console.log("stat present:", present !== undefined && present.isFile());
for (const opts of [{ throwIfNoEntry: true }, {}, undefined]) {
  try {
    fs.statSync(missing, opts as any);
    console.log("no throw");
  } catch (e: any) {
    console.log("throws:", e.code);
  }
}
try {
  fs.lstatSync(missing, { throwIfNoEntry: true });
} catch (e: any) {
  console.log("lstat throws:", e.code);
}
fs.rmSync(ROOT, { recursive: true, force: true });
