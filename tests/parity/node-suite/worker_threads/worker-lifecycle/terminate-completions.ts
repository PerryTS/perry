import { Worker } from "node:worker_threads";

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./termination-worker.cjs");
let exits = 0;
worker.on("online", async () => {
  const first = worker.terminate();
  const second = worker.terminate();
  const third = worker.terminate();
  console.log("distinct:", first !== second && second !== third && first !== third);
  console.log("results:", await first, await second, await third);
  console.log("later:", String(await worker.terminate()), "exits:", exits);
});
worker.on("exit", (code) => {
  exits++;
  console.log("exit:", code);
  worker.terminate().then((result) => console.log("inside exit:", String(result)));
});
