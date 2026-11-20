import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_opendir_concurrent_operation";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_err) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/a.txt", "A");
fs.writeFileSync(ROOT + "/b.txt", "B");

function firstLine(err: any): string {
  return String(err?.message || "").split("\n")[0];
}

function check(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label + " OK:", value === undefined ? "undefined" : String(value));
  } catch (err: any) {
    console.log(label + " THROW:", err?.name, err?.code || "no-code", firstLine(err));
  }
}

const readDir = fs.opendirSync(ROOT);
const readPromise = readDir.read();
check("closeSync while read pending", () => readDir.closeSync());
check("readSync while read pending", () => readDir.readSync()?.name ? "entry" : null);
const readEntry = await readPromise;
console.log("read promise settled:", readEntry?.name ? "entry" : String(readEntry));
check("closeSync after read settled", () => readDir.closeSync());

const callbackDir = fs.opendirSync(ROOT);
await new Promise<void>((resolve) => {
  callbackDir.read((err: any, entry: fs.Dirent | null) => {
    console.log("callback read settled:", err?.code || (entry?.name ? "entry" : String(entry)));
    check("readSync inside callback", () => callbackDir.readSync()?.name ? "entry" : null);
    check("closeSync inside callback", () => callbackDir.closeSync());
    resolve();
  });
  check("closeSync after callback read call", () => callbackDir.closeSync());
  check("readSync after callback read call", () => callbackDir.readSync()?.name ? "entry" : null);
});
check("closeSync after callback settled", () => callbackDir.closeSync());

const closeDir = fs.opendirSync(ROOT);
const closePromise = closeDir.close();
check("readSync after close scheduled", () => closeDir.readSync()?.name ? "entry" : null);
check("closeSync after close scheduled", () => closeDir.closeSync());
await closePromise;

try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_err) {}
