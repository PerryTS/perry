import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_stats_timestamp_precision";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const file = ROOT + "/time.txt";
await fsp.writeFile(file, "time");

const expectedMs = Date.UTC(2020, 0, 2, 3, 4, 5, 678);
fs.utimesSync(file, expectedMs / 1000, expectedMs / 1000);

const st = await fsp.stat(file);
const big = await fsp.stat(file, { bigint: true });

const nsInMsWindow = (ns: bigint, ms: bigint) =>
  ns >= ms * 1000000n && ns < (ms + 1n) * 1000000n;

console.log("promises timestamp numeric ms types:", ["atimeMs", "mtimeMs", "ctimeMs", "birthtimeMs"].map((k) => typeof (st as any)[k]).join(","));
console.log("promises timestamp date alias types:", st.atime instanceof Date, st.mtime instanceof Date, st.ctime instanceof Date, st.birthtime instanceof Date);
console.log("promises timestamp mtime set:", Math.abs(st.mtimeMs - expectedMs) < 2);
console.log("promises timestamp date mtime mirrors ms:", Math.abs(st.mtime.getTime() - Math.round(st.mtimeMs)) <= 1);

console.log("promises timestamp bigint ms types:", ["atimeMs", "mtimeMs", "ctimeMs", "birthtimeMs"].map((k) => typeof (big as any)[k]).join(","));
console.log("promises timestamp bigint ns types:", ["atimeNs", "mtimeNs", "ctimeNs", "birthtimeNs"].map((k) => typeof (big as any)[k]).join(","));
console.log("promises timestamp bigint mtime near numeric:", Math.abs(Number(big.mtimeMs) - st.mtimeMs) < 2);
console.log("promises timestamp bigint mtime ns relation:", nsInMsWindow(big.mtimeNs, big.mtimeMs));
console.log("promises timestamp bigint date mirrors ms:", Math.abs(big.mtime.getTime() - Number(big.mtimeMs)) <= 1);
