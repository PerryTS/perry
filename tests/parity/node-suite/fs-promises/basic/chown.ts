import * as fs from "node:fs";
import * as os from "node:os";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_chown";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";
const link = ROOT + "/link.txt";
await fsp.writeFile(p, "owner");
await fsp.symlink(p, link);
const info = os.userInfo();

await fsp.chown(p, info.uid, info.gid);
console.log("promises chown content:", await fsp.readFile(p, "utf8"));
await (fsp as any).lchown(link, info.uid, info.gid);
console.log("promises lchown link:", (await fsp.lstat(link)).isSymbolicLink());
const handle = await fsp.open(p, "r+");
await handle.chown(info.uid, info.gid);
await handle.close();
console.log("filehandle chown exists:", fs.existsSync(p));
