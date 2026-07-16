import * as workerThreads from "node:worker_threads";
import { isInternalThread, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

console.log(
  "main:",
  isInternalThread,
  workerThreads.isInternalThread,
  workerThreads.isMainThread,
);

const worker = new Worker("./internal-thread-worker.cjs");
worker.on("message", (value: any) => {
  console.log("worker:", value.named, value.namespace, value.main);
});
worker.on("error", (error: any) => {
  console.log("error:", error?.name, error?.code ?? "");
});
worker.on("exit", (code) => console.log("exit:", code));
