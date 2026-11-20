// #3899: `workerData` is a value-only export (null on the main thread), not a
// callable method — `typeof workerData === "object"` and `workerData()` throws.
import * as wt from "node:worker_threads";
function codeOf(fn: () => unknown): string {
  try { const v = fn(); return `ok:${typeof v}:${String(v)}`; }
  catch (err: any) { return `${err?.name ?? "Error"}`; }
}
console.log("workerData:", typeof wt.workerData, String(wt.workerData));
console.log("workerData()", codeOf(() => (wt as any).workerData()));
console.log("parentPort:", typeof wt.parentPort, String(wt.parentPort));
console.log("isMainThread:", wt.isMainThread, "threadId:", wt.threadId);
