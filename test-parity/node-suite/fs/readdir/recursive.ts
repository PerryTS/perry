import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readdir_recursive";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/a/b", { recursive: true });
fs.writeFileSync(ROOT + "/root.txt", "root");
fs.writeFileSync(ROOT + "/a/one.txt", "one");
fs.writeFileSync(ROOT + "/a/b/two.txt", "two");

const syncNames = fs.readdirSync(ROOT, { recursive: true }).sort();
console.log("recursive sync:", syncNames.join(","));
fs.readdir(ROOT, { recursive: true }, (err, names) => {
  console.log("recursive callback err:", err === null);
  console.log("recursive callback:", names.sort().join(","));
});
