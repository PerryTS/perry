// fs errors carry libuv's code for every errno: an errno outside a short list of
// common ones (ENXIO here) must not be reported as EIO.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "errno-codes-"));
function show(label: string, fn: () => unknown): void {
  try {
    fn();
    console.log(label, "no error");
  } catch (e: any) {
    console.log(label, e.code, e.errno, e.syscall, String(e.message).split(dir).join("<dir>"));
  }
}

const fifo = path.join(dir, "fifo");
execFileSync("mkfifo", [fifo]);
// Opening a FIFO write-only and non-blocking with no reader fails with ENXIO.
show("ENXIO", () => fs.openSync(fifo, fs.constants.O_WRONLY | fs.constants.O_NONBLOCK));
show("ENOENT", () => fs.openSync(path.join(dir, "missing"), "r"));
show("EISDIR", () => fs.readFileSync(dir));
const file = path.join(dir, "file");
fs.writeFileSync(file, "x");
show("ENOTDIR", () => fs.openSync(path.join(file, "below"), "r"));
fs.symlinkSync(path.join(dir, "loop-b"), path.join(dir, "loop-a"));
fs.symlinkSync(path.join(dir, "loop-a"), path.join(dir, "loop-b"));
show("ELOOP", () => fs.openSync(path.join(dir, "loop-a"), "r"));
show("EEXIST", () => fs.mkdirSync(dir));
show("ENOTEMPTY", () => fs.rmdirSync(dir));
// Permission bits do not stop root, so EACCES is only observable for other users.
if (process.getuid && process.getuid() !== 0) {
  const locked = path.join(dir, "locked");
  fs.writeFileSync(locked, "x");
  fs.chmodSync(locked, 0o000);
  show("EACCES", () => fs.readFileSync(locked));
  fs.chmodSync(locked, 0o600);
}
fs.rmSync(dir, { recursive: true });
