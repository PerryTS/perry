import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_rm_symlink_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const targetDir = ROOT + "/target-dir";
const linkDir = ROOT + "/link-dir";
await fsp.mkdir(targetDir);
await fsp.writeFile(targetDir + "/keep.txt", "promises-keep");
await fsp.symlink(targetDir, linkDir, "dir");
await fsp.rm(linkDir, { recursive: true });
console.log("promises rm symlink removed:", !fs.existsSync(linkDir));
console.log("promises rm target kept:", await fsp.readFile(targetDir + "/keep.txt", "utf8"));

await fsp.rm(ROOT + "/missing", { force: true });
console.log("promises rm missing force ok:", true);
