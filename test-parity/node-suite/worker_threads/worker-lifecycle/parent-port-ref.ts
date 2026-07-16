import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./parent-port-ref-worker.cjs");
worker.on("message", (message) => {
  console.log(
    "states:",
    message.initial,
    message.unrefReturn,
    message.unrefed,
    message.refReturn,
    message.refed,
  );
  worker.terminate().then((code) => console.log("terminate:", code));
});
worker.on("exit", (code) => console.log("exit:", code));
