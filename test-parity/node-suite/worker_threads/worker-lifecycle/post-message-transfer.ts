import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./post-transfer-worker.cjs");
const buffer = new ArrayBuffer(4);
const view = new Uint8Array(buffer);
view.set([2, 4, 6, 8]);

worker.on("online", () => {
  worker.postMessage({ view }, [buffer]);
  console.log("source detached:", buffer.byteLength, view.byteLength);
});
worker.on("message", (message: any) => {
  console.log("worker received:", message?.brand, message?.length, message?.values);
  worker.terminate();
});
worker.on("exit", (code) => console.log("exit:", code));
