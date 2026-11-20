import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_dirent_predicates";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.mkdirSync(ROOT + "/dir");
fs.writeFileSync(ROOT + "/file.txt", "f");
const entries = fs.readdirSync(ROOT, { withFileTypes: true }).slice().sort((a, b) => a.name.localeCompare(b.name));
const predicates = ["isFile", "isDirectory", "isSymbolicLink", "isBlockDevice", "isCharacterDevice", "isFIFO", "isSocket"];

for (const [label, entry] of [["dir", entries[0]], ["file", entries[1]]] as const) {
  for (const predicate of predicates) {
    const fn = (entry as any)[predicate];
    console.log(`${label} ${predicate} type:`, typeof fn);
    console.log(`${label} ${predicate}:`, fn.call(entry));
  }
}
