import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");
const worker = new Worker("./global-postmessage-worker.cjs");
worker.on("message", (message) => {
  if (message === "ready") {
    worker.postMessage("value");
    return;
  }
  console.log("message:", message);
  worker.terminate();
});
worker.on("error", (error) => {
  console.log("error:", error.name, (error as any).code ?? "");
});
worker.on("exit", (code) => console.log("exit:", code));
