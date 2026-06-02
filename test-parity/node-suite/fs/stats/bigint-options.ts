import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_bigint_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
fs.writeFileSync(file, "hello");

const st = fs.statSync(file, { bigint: true });
console.log("statSync bigint size type:", typeof st.size);
console.log("statSync bigint mode type:", typeof st.mode);
console.log("statSync bigint nlink type:", typeof st.nlink);
console.log("statSync bigint predicate:", st.isFile());

const link = ROOT + "/link.txt";
fs.symlinkSync("file.txt", link);
const lst = fs.lstatSync(link, { bigint: true });
console.log("lstatSync bigint size type:", typeof lst.size);
console.log("lstatSync bigint symlink:", lst.isSymbolicLink());

const fd = fs.openSync(file, "r");
const fst = fs.fstatSync(fd, { bigint: true });
console.log("fstatSync bigint size type:", typeof fst.size);
fs.closeSync(fd);

fs.stat(file, { bigint: true }, (err, cst) => {
  console.log("stat callback bigint err:", err === null);
  console.log("stat callback bigint size type:", typeof cst.size);
});
