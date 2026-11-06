import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_open_flags";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";

let fh = await fsp.open(p, "wx");
console.log("promises open wx fd:", fh.fd >= 0);
await fh.write("one");
await fh.close();
console.log("promises open wx content:", await fsp.readFile(p, "utf8"));

fh = await fsp.open(p, "a+");
await fh.write(" two");
await fh.close();
console.log("promises open a+ content:", await fsp.readFile(p, "utf8"));
