import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./process-env-descriptor-worker.cjs");
worker.on("message", (value: any) => {
  console.log("descriptors:", value.join("|"));
});
worker.on("error", (error: any) => {
  console.log("error:", error?.name, error?.code ?? "");
});
worker.on("exit", (code) => console.log("exit:", code));
