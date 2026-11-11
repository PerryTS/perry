import { Worker, isMainThread, parentPort } from "node:worker_threads";
import { fileURLToPath } from "node:url";
// ESM supplies the Node filename binding explicitly.
const __filename = fileURLToPath(import.meta.url);
if (isMainThread) {
    const w = new Worker(__filename);
    w.on("message", (m: number) => { console.log("filename", m); w.terminate(); });
    w.postMessage(21);
} else {
    parentPort!.on("message", (n: number) => parentPort!.postMessage(n * 2));
}
