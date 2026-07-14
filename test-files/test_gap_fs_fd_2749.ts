// Gap test: node:fs fd-mutator error propagation for ftruncate / fchown /
// futimes (#2749). Operating on a closed/invalid fd must surface EBADF with
// Node-compatible `err.code`/`err.syscall` across the sync, callback, and
// promisified (await) forms instead of silently succeeding.
//
// We assert only `err.code` and `err.syscall` (plus success markers), never
// volatile message text. Node names the futimes syscall "futime" (singular).
import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { promisify } from "node:util";

const dir = "/tmp/perry_fs_fd_2749";
fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });

const file = dir + "/file.txt";
fs.writeFileSync(file, "hello world");

// Open then immediately close to confirm the descriptor lifecycle works;
// the resulting fd number can be recycled by internal opens (libuv, the
// promisify machinery), so the actual error assertions below operate on a
// deterministically-invalid descriptor that was never allocated.
const opened = fs.openSync(file, "r+");
fs.closeSync(opened);
const fd = 987654321;

function syncCode(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": OK");
  } catch (e: any) {
    console.log(label + ": " + e.code + " " + e.syscall);
  }
}

// The three callback-form calls below are dispatched to the libuv threadpool
// concurrently, so their callbacks fire in *completion* order, which is not
// deterministic — node itself emits `fchown closed cb` before `futimes closed
// cb` on roughly one run in six. Comparing raw interleaving against a freshly
// run node oracle therefore fails at random on any PR.
//
// What this test is actually about is the error surface (`err.code` /
// `err.syscall`) of each fd mutator, not libuv's scheduling. So buffer the
// three results and flush them in a fixed order once all three have landed.
// Every assertion is preserved; only the print order is pinned.
const CB_ORDER = ["ftruncate closed cb", "futimes closed cb", "fchown closed cb"];
const cbLines = new Map<string, string>();

function cbCode(label: string, err: any): void {
  cbLines.set(label, err ? label + ": " + err.code + " " + err.syscall : label + ": OK");
  if (cbLines.size < CB_ORDER.length) {
    return;
  }
  for (const key of CB_ORDER) {
    console.log(cbLines.get(key));
  }
}

async function asyncCode(label: string, fn: () => Promise<unknown>): Promise<void> {
  try {
    await fn();
    console.log(label + ": OK");
  } catch (e: any) {
    console.log(label + ": " + e.code + " " + e.syscall);
  }
}

// --- sync forms ---
syncCode("ftruncate closed sync", () => fs.ftruncateSync(fd, 1));
syncCode("futimes closed sync", () => fs.futimesSync(fd, 1, 2));
syncCode("fchown closed sync", () => fs.fchownSync(fd, 0, 0));

// keep `fsp` referenced so the namespace import is exercised
console.log("fsp.readFile typeof=" + typeof fsp.readFile);

// --- callback forms ---
fs.ftruncate(fd, 1, (err: any) => cbCode("ftruncate closed cb", err));
fs.futimes(fd, 1, 2, (err: any) => cbCode("futimes closed cb", err));
fs.fchown(fd, 0, 0, (err: any) => cbCode("fchown closed cb", err));

// --- promisified (await) forms ---
const ftruncateP = promisify(fs.ftruncate);
const futimesP = promisify(fs.futimes);
const fchownP = promisify(fs.fchown);

async function main(): Promise<void> {
  await asyncCode("ftruncate closed promise", () => ftruncateP(fd, 1));
  await asyncCode("futimes closed promise", () => futimesP(fd, 1, 2));
  await asyncCode("fchown closed promise", () => fchownP(fd, 0, 0));

  fs.rmSync(dir, { recursive: true, force: true });
}

main();
