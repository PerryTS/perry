// Runtime dispatch uses the same entry initializer as a statically resolved Worker.
import { isMainThread, parentPort, workerData } from "node:worker_threads";
if (isMainThread) {
    const ns = (globalThis as any).process.getBuiltinModule("node:worker_threads");
    const entry = new URL("./test_gap_worker_self_entry_runtime.ts", import.meta.url);
    const w = new ns.Worker(entry, { workerData: 2 });
    w.on("message", (m: number) => { console.log("runtime", m); w.terminate(); });
    w.postMessage(21);
} else {
    parentPort!.on("message", (n: number) => parentPort!.postMessage(n * workerData));
}
