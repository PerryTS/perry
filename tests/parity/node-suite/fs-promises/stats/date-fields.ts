import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_stats_date_fields";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
const link = ROOT + "/link.txt";
await fsp.writeFile(file, "date-fields");
try { await fsp.symlink(file, link); } catch (_e) {}

const oldMs = Date.parse("2005-06-07T08:09:10.000Z");
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

const st = await fsp.stat(file);
console.log(
  "promises Date aliases enumerable:",
  ["atime", "mtime", "ctime", "birthtime"].map((key) => Object.keys(st).includes(key)).join(","),
);
console.log(
  "promises Date aliases own:",
  ["atime", "mtime", "ctime", "birthtime"].map((key) => Object.prototype.hasOwnProperty.call(st, key)).join(","),
);
show("promises stat", st, oldMs);

const lst = await fsp.lstat(link);
show("promises lstat", lst);

const originalMtime = st.mtime;
st.mtimeMs = 789;
console.log(
  "promises Date independent from mtimeMs:",
  originalMtime instanceof Date && st.mtime === originalMtime && st.mtime.getTime() === oldMs && st.mtimeMs === 789,
);
st.mtime = new Date(654);
console.log(
  "promises mtimeMs independent from Date:",
  st.mtime instanceof Date && st.mtime.getTime() === 654 && st.mtimeMs === 789,
);

const fh = await fsp.open(file, "r");
const fhSt = await fh.stat();
show("filehandle stat", fhSt, oldMs);
const fhBig = await fh.stat({ bigint: true });
show("filehandle bigint", fhBig, oldMs);
console.log("filehandle bigint timestamp types:", `${typeof fhBig.mtimeMs},${typeof fhBig.mtimeNs}`);
await fh.close();
