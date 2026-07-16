import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const events: string[] = [];
const worker = new Worker("./natural-exit-worker.cjs");
worker.on("online", () => events.push("online"));
worker.on("message", (message) => events.push(`message:${message}`));
worker.on("exit", (code) => {
  events.push(`exit:${code}`);
  worker.terminate().then((terminateCode) => {
    events.push(`terminate:${String(terminateCode)}`);
    console.log("events:", events.join(","));
  });
});
