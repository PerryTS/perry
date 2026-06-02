import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_date_fields";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
const link = ROOT + "/link.txt";
fs.writeFileSync(file, "date-fields");
try { fs.symlinkSync(file, link); } catch (_e) {}

const oldMs = Date.parse("2004-05-06T07:08:09.000Z");
fs.utimesSync(file, oldMs / 1000, oldMs / 1000);

function show(label: string, st: any, expectedMtimeMs?: number) {
  console.log(
    `${label} date aliases:`,
    `${st.atime instanceof Date},${st.mtime instanceof Date},${st.ctime instanceof Date},${st.birthtime instanceof Date}`,
  );
  if (expectedMtimeMs !== undefined) {
    console.log(
      `${label} mtime close:`,
      st.mtime instanceof Date && Math.abs(st.mtime.getTime() - expectedMtimeMs) < 2000,
    );
  }
}

const st = fs.statSync(file);
console.log(
  "statSync Date aliases enumerable:",
  ["atime", "mtime", "ctime", "birthtime"].map((key) => Object.keys(st).includes(key)).join(","),
);
console.log(
  "statSync Date aliases own:",
  ["atime", "mtime", "ctime", "birthtime"].map((key) => Object.prototype.hasOwnProperty.call(st, key)).join(","),
);
show("statSync", st, oldMs);

const lst = fs.lstatSync(link);
show("lstatSync", lst);

const fd = fs.openSync(file, "r");
const fst = fs.fstatSync(fd);
show("fstatSync", fst, oldMs);
fs.closeSync(fd);

const originalMtime = st.mtime;
st.mtimeMs = 123;
console.log(
  "statSync Date independent from mtimeMs:",
  originalMtime instanceof Date && st.mtime === originalMtime && st.mtime.getTime() === oldMs && st.mtimeMs === 123,
);
st.mtime = new Date(456);
console.log(
  "statSync mtimeMs independent from Date:",
  st.mtime instanceof Date && st.mtime.getTime() === 456 && st.mtimeMs === 123,
);

const big = fs.statSync(file, { bigint: true });
show("statSync bigint", big, oldMs);
console.log("statSync bigint timestamp types:", `${typeof big.mtimeMs},${typeof big.mtimeNs}`);

fs.stat(file, (err, cbSt) => {
  console.log("stat callback err:", err === null);
  show("stat callback", cbSt, oldMs);
});
