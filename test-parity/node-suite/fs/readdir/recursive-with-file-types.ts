import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readdir_recursive_dirents";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/a", { recursive: true });
fs.writeFileSync(ROOT + "/root.txt", "root");
fs.writeFileSync(ROOT + "/a/child.txt", "child");

const entries = fs.readdirSync(ROOT, { recursive: true, withFileTypes: true });
for (const ent of entries) {
  const parent = (ent as any).parentPath || (ent as any).path;
  const parentName = parent === ROOT ? "." : parent.slice(ROOT.length + 1);
  console.log("recursive dirent:", ent.name, parentName, ent.isDirectory(), ent.isFile());
}
