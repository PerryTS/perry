import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_stats_bigint_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
await fsp.writeFile(file, "hello");

const st = await fsp.stat(file, { bigint: true });
console.log("promises stat bigint size type:", typeof st.size);
console.log("promises stat bigint mode type:", typeof st.mode);
console.log("promises stat bigint predicate:", st.isFile());

const link = ROOT + "/link.txt";
await fsp.symlink("file.txt", link);
const lst = await fsp.lstat(link, { bigint: true });
console.log("promises lstat bigint size type:", typeof lst.size);
console.log("promises lstat bigint symlink:", lst.isSymbolicLink());

const fh = await fsp.open(file, "r");
const fst = await fh.stat({ bigint: true });
console.log("filehandle stat bigint size type:", typeof fst.size);
await fh.close();
