import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_timestamp_precision";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/time.txt";
fs.writeFileSync(file, "time");

const expectedMs = Date.UTC(2020, 0, 2, 3, 4, 5, 678);
fs.utimesSync(file, expectedMs / 1000, expectedMs / 1000);

const st = fs.statSync(file);
const big = fs.statSync(file, { bigint: true });

const nsInMsWindow = (ns: bigint, ms: bigint) =>
  ns >= ms * 1000000n && ns < (ms + 1n) * 1000000n;

console.log("timestamp numeric ms types:", ["atimeMs", "mtimeMs", "ctimeMs", "birthtimeMs"].map((k) => typeof (st as any)[k]).join(","));
console.log("timestamp date alias types:", st.atime instanceof Date, st.mtime instanceof Date, st.ctime instanceof Date, st.birthtime instanceof Date);
console.log("timestamp mtime set:", Math.abs(st.mtimeMs - expectedMs) < 2);
console.log("timestamp date mtime mirrors ms:", Math.abs(st.mtime.getTime() - Math.round(st.mtimeMs)) <= 1);

console.log("timestamp bigint ms types:", ["atimeMs", "mtimeMs", "ctimeMs", "birthtimeMs"].map((k) => typeof (big as any)[k]).join(","));
console.log("timestamp bigint ns types:", ["atimeNs", "mtimeNs", "ctimeNs", "birthtimeNs"].map((k) => typeof (big as any)[k]).join(","));
console.log("timestamp bigint mtime near numeric:", Math.abs(Number(big.mtimeMs) - st.mtimeMs) < 2);
console.log("timestamp bigint mtime ns relation:", nsInMsWindow(big.mtimeNs, big.mtimeMs));
console.log("timestamp bigint date mirrors ms:", Math.abs(big.mtime.getTime() - Number(big.mtimeMs)) <= 1);
