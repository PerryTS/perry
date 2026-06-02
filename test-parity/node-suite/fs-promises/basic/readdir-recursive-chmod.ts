import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_readdir_recursive_chmod";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT + "/a/b", { recursive: true });
await fsp.writeFile(ROOT + "/a/b/two.txt", "two");
await fsp.writeFile(ROOT + "/one.txt", "one");
const names = await fsp.readdir(ROOT, { recursive: true });
console.log("promises recursive:", names.sort().join(","));
await fsp.chmod(ROOT + "/one.txt", 0o600);
const st = await fsp.stat(ROOT + "/one.txt");
console.log("promises chmod mode:", (st.mode & 0o777).toString(8));
