import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_promises_cp_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const src = ROOT + "/src.txt";
const dst = ROOT + "/dst.txt";
await fsp.writeFile(src, "newer");
await fsp.writeFile(dst, "older");
await fsp.cp(src, dst, { force: false });
console.log("promises cp force false:", await fsp.readFile(dst, "utf8"));
await fsp.cp(src, dst, { force: true });
console.log("promises cp force true:", await fsp.readFile(dst, "utf8"));

await fsp.utimes(src, 1118131750, 1152349811);
await fsp.cp(src, ROOT + "/preserved.txt", { preserveTimestamps: true });
const st = await fsp.stat(ROOT + "/preserved.txt");
console.log("promises cp preserve timestamp seconds:", Math.floor(st.mtimeMs / 1000));

await fsp.writeFile(ROOT + "/target.txt", "linked");
await fsp.symlink("target.txt", ROOT + "/link.txt");
await fsp.cp(ROOT + "/link.txt", ROOT + "/link-copy.txt", { dereference: true });
console.log("promises cp dereference:", await fsp.readFile(ROOT + "/link-copy.txt", "utf8"));

await fsp.rm(ROOT + "/missing", { force: true });
console.log("promises rm missing force:", fs.existsSync(ROOT + "/missing"));
