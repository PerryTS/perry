import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cp";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.mkdirSync(ROOT + "/src");
fs.mkdirSync(ROOT + "/src/nested");
fs.writeFileSync(ROOT + "/src/a.txt", "A");
fs.writeFileSync(ROOT + "/src/nested/b.txt", "B");
fs.cpSync(ROOT + "/src", ROOT + "/dst", { recursive: true });
console.log("cp root file:", fs.readFileSync(ROOT + "/dst/a.txt", "utf8"));
console.log("cp nested file:", fs.readFileSync(ROOT + "/dst/nested/b.txt", "utf8"));
