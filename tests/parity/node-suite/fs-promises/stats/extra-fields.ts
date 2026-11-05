import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_stats_extra_fields";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
await fsp.writeFile(file, "extra-fields");

const oldMs = Date.parse("2005-06-07T08:09:10.000Z");
fs.utimesSync(file, oldMs / 1000, oldMs / 1000);

const st = await fsp.stat(file);
console.log("promises stats extra mtimeMs number:", typeof st.mtimeMs);
console.log("promises stats extra platform field types:", ["dev", "rdev", "blksize", "ino", "blocks"].map((k) => typeof (st as any)[k]).join(","));
console.log("promises stats extra mtime old:", Math.abs(st.mtimeMs - oldMs) < 2000);

const big = await fsp.stat(file, { bigint: true });
console.log("promises stats extra bigint field types:", ["dev", "rdev", "blksize", "ino", "blocks", "mtimeNs"].map((k) => typeof (big as any)[k]).join(","));
console.log("promises stats extra bigint ns relation:", big.mtimeNs >= big.mtimeMs * 1000000n);

const fh = await fsp.open(file, "r");
const fhSt = await fh.stat({ bigint: true });
console.log("promises filehandle stat extra bigint:", typeof fhSt.ino + "," + typeof fhSt.mtimeNs);
await fh.close();
