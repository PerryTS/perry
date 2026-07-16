import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./process-unsupported-worker.cjs");
worker.on("message", (message) => {
  console.log("disabled:", message.chdirDisabled, message.abortDisabled);
  console.log("chdir:", message.chdir);
  console.log("umask:", message.umask);
});
worker.on(
  "error",
  (error: any) => console.log("error:", error?.name, error?.code ?? ""),
);
worker.on("exit", (code) => console.log("exit:", code));
