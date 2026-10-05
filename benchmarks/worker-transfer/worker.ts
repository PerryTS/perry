import { parentPort } from "node:worker_threads";
parentPort!.on("message", (ab: ArrayBuffer) => {
    parentPort!.postMessage(ab, [ab]);
    if (ab.byteLength !== 0) process.exit(5);
});
