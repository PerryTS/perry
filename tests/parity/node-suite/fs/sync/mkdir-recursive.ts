import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_mkdir_recursive";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/a/b/c", { recursive: true });
fs.writeFileSync(ROOT + "/a/b/c/file.txt", "nested");
console.log("nested dir exists:", fs.statSync(ROOT + "/a/b/c").isDirectory());
console.log("nested content:", fs.readFileSync(ROOT + "/a/b/c/file.txt", "utf8"));
