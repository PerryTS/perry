import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_readdir_recursive_dirents";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT + "/a", { recursive: true });
await fsp.writeFile(ROOT + "/root.txt", "root");
await fsp.writeFile(ROOT + "/a/child.txt", "child");

const entries = await fsp.readdir(ROOT, { recursive: true, withFileTypes: true });
for (const ent of entries) {
  const parent = (ent as any).parentPath || (ent as any).path;
  const parentName = parent === ROOT ? "." : parent.slice(ROOT.length + 1);
  console.log("promises recursive dirent:", ent.name, parentName, ent.isDirectory(), ent.isFile());
}
