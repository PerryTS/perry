import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_opendir_invalid_paths";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const file = ROOT + "/file.txt";
const missing = ROOT + "/missing";
fs.writeFileSync(file, "x");

function probe(label: string, fn: () => any) {
  try {
    const value = fn();
    console.log(label, "no-throw", !!value);
    if (value) value.closeSync();
  } catch (err: any) {
    console.log(label, err.name, err.code || "", err.syscall || "");
  }
}

probe("opendirSync missing", () => fs.opendirSync(missing));
probe("opendirSync file", () => fs.opendirSync(file));
probe("opendirSync null", () => fs.opendirSync(null as any));
probe("opendirSync object", () => fs.opendirSync({} as any));

const dir = fs.opendirSync(ROOT);
console.log("opendirSync valid path:", dir.path === ROOT);
dir.closeSync();

await new Promise<void>((resolve) => {
  fs.opendir(missing, (err, dir) => {
    console.log(
      "opendir callback missing",
      err && err.name,
      err && (err as any).code,
      err && (err as any).syscall,
      dir === undefined,
    );
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.opendir(file, (err, dir) => {
    console.log(
      "opendir callback file",
      err && err.name,
      err && (err as any).code,
      err && (err as any).syscall,
      dir === undefined,
    );
    resolve();
  });
});
