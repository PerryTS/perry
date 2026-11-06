import { Worker, isMainThread, parentPort } from "node:worker_threads";
import { fileURLToPath } from "node:url";
if (isMainThread) {
    const w = new Worker(fileURLToPath(import.meta.url));
    w.on("message", (m) => { console.log("path", m); w.terminate(); });
    w.postMessage(21);
} else {
    parentPort!.on("message", (n: number) => parentPort!.postMessage(n * 2));
}
