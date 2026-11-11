import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cp_filter";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/src/a", { recursive: true });
fs.writeFileSync(ROOT + "/src/keep.txt", "keep");
fs.writeFileSync(ROOT + "/src/drop.md", "drop");
fs.writeFileSync(ROOT + "/src/a/nested.txt", "nested");
fs.writeFileSync(ROOT + "/src/a/nested.md", "nested drop");

let calls = 0;
fs.cpSync(ROOT + "/src", ROOT + "/dst", {
  recursive: true,
  filter: (src) => {
    calls++;
    return fs.statSync(src).isDirectory() || src.endsWith(".txt");
  },
});
console.log("filter called:", calls > 0);
console.log("keep copied:", fs.existsSync(ROOT + "/dst/keep.txt"));
console.log("drop skipped:", fs.existsSync(ROOT + "/dst/drop.md"));
console.log("nested keep copied:", fs.existsSync(ROOT + "/dst/a/nested.txt"));
console.log("nested drop skipped:", fs.existsSync(ROOT + "/dst/a/nested.md"));
