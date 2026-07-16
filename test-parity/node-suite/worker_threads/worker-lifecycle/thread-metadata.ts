import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./thread-metadata-worker.cjs", {
  name: "parity-worker",
});

console.log("parent initial:", worker.threadId > 0, worker.threadName);
worker.on("online", () => {
  console.log("online:", worker.threadId > 0, worker.threadName);
});
worker.on("message", (message) => {
  console.log(
    "worker values:",
    message.threadId === worker.threadId,
    message.threadName,
  );
});
worker.on("exit", (code) => {
  console.log("exit:", code, worker.threadId, worker.threadName);
});
