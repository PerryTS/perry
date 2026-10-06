// #12092: the program entry must also run as a worker module.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
    const w = new Worker(new URL(import.meta.url));
    w.on("message", (m) => { console.log("got", m); w.terminate(); });
    w.postMessage(21);
} else {
    parentPort!.on("message", (n: number) => parentPort!.postMessage(n * 2));
}
