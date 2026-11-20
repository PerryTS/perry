import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stat_missing_errors";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_err) {}

function probe(label: string, path: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(label, err.name, err.code, err.syscall, err.path === path);
  }
}

probe("statSync missing", ROOT + "/missing-stat.txt", () =>
  fs.statSync(ROOT + "/missing-stat.txt"),
);
probe("lstatSync missing", ROOT + "/missing-lstat.txt", () =>
  fs.lstatSync(ROOT + "/missing-lstat.txt"),
);

fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
fs.writeFileSync(file, "ok");

console.log("statSync success:", fs.statSync(file).isFile());
console.log("lstatSync success:", fs.lstatSync(file).isFile());
