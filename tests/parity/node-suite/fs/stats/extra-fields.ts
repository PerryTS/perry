import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_extra_fields";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
fs.writeFileSync(file, "extra-fields");

const oldMs = Date.parse("2004-05-06T07:08:09.000Z");
fs.utimesSync(file, oldMs / 1000, oldMs / 1000);

const st = fs.statSync(file);
console.log("stats extra mtimeMs number:", typeof st.mtimeMs);
console.log("stats extra platform field types:", ["dev", "rdev", "blksize", "ino", "blocks"].map((k) => typeof (st as any)[k]).join(","));
console.log("stats extra mtime old:", Math.abs(st.mtimeMs - oldMs) < 2000);

const bigSt = fs.statSync(file, { bigint: true });
console.log("stats extra bigint field types:", ["dev", "rdev", "blksize", "ino", "blocks", "mtimeNs"].map((k) => typeof (bigSt as any)[k]).join(","));
console.log("stats extra bigint ns relation:", bigSt.mtimeNs >= bigSt.mtimeMs * 1000000n);

fs.stat(file, (err, cbSt) => {
  console.log("stats extra callback err:", err === null);
  console.log("stats extra callback platform fields:", typeof (cbSt as any).dev + "," + typeof (cbSt as any).ino);
});
