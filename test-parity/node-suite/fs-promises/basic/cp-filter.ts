import * as fsp from "node:fs/promises";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fsp_cp_filter";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT + "/src/a", { recursive: true });
await fsp.writeFile(ROOT + "/src/keep.txt", "keep");
await fsp.writeFile(ROOT + "/src/drop.md", "drop");
await fsp.writeFile(ROOT + "/src/a/nested.txt", "nested");
await fsp.writeFile(ROOT + "/src/a/nested.md", "nested drop");

let calls = 0;
await fsp.cp(ROOT + "/src", ROOT + "/dst", {
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
