import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";
if (isMainThread) {
    const w = new Worker(new URL(import.meta.url), { workerData: { factor: 3 } });
    w.on("message", (m) => { console.log("data", m); w.terminate(); });
    w.postMessage(14);
} else {
    parentPort!.on("message", (n: number) => parentPort!.postMessage(n * workerData.factor));
}
