import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fstat_invalid_fd";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const path = ROOT + "/file.txt";
fs.writeFileSync(path, "abc");

function probe(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(label, err.name, err.code || "", err.syscall || "");
  }
}

probe("fstatSync negative", () => fs.fstatSync(-1));
probe("fstatSync fractional", () => fs.fstatSync(1.5));
probe("fstatSync string", () => fs.fstatSync("x" as any));
probe("fstatSync bad fd", () => fs.fstatSync(99999999));

const fd = fs.openSync(path, "r");
const stats = fs.fstatSync(fd, { bigint: true });
console.log("fstatSync valid bigint:", typeof stats.size, stats.isFile());
fs.closeSync(fd);

probe("fstat callback fractional", () => {
  fs.fstat(1.5, () => {});
});

await new Promise<void>((resolve) => {
  fs.fstat(99999999, (err, stats) => {
    console.log(
      "fstat callback bad fd",
      err && err.name,
      err && (err as any).code,
      err && (err as any).syscall,
      stats === undefined,
    );
    resolve();
  });
});
