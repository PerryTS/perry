// Await really suspends on a worker-owned timer before installing the listener.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
    const w = new Worker(new URL(import.meta.url));
    await new Promise<void>((resolve) => {
        w.on("message", (m) => {
            if (m === "ready") w.postMessage(21);
            else { console.log("await", m); w.terminate().then(() => resolve()); }
        });
    });
} else {
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
    parentPort!.on("message", (n: number) => parentPort!.postMessage(n * 2));
    parentPort!.postMessage("ready");
}
