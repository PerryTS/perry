import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const shared = new SharedArrayBuffer(4);
const view = new Uint8Array(shared);
view.set([1, 2, 3, 4]);

const worker = new Worker("./workerdata-shared-array-buffer-worker.cjs", {
  workerData: { shared },
});
worker.on("message", (message) => {
  console.log("worker:", message.brand, message.before, message.after);
  console.log("parent shared:", Array.from(view).join(","));
});
worker.on("exit", (code) => console.log("exit:", code));
