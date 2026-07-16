import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./termination-worker.cjs");
worker.on("message", (message) => {
  console.log("ready:", message);
  console.log("unref return:", worker.unref() === worker);
  console.log("ref return:", worker.ref() === worker);
  worker.terminate().then((code) => console.log("terminate:", code));
});
worker.on("exit", (code) => console.log("exit:", code));
