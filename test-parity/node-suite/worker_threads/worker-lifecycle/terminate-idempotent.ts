import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./termination-worker.cjs");
worker.on("online", async () => {
  const first = worker.terminate();
  const second = worker.terminate();
  console.log("promise identity:", first === second);
  console.log("results:", await first, await second);
});
worker.on("exit", (code) => console.log("exit:", code));
