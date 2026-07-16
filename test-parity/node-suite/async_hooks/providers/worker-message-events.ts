import { Worker } from "node:worker_threads";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
process.chdir("test-parity/node-suite/async_hooks/providers");

const result = await storage.run(
  "worker-events",
  () =>
    new Promise<string>((resolve, reject) => {
      const worker = new Worker("./fixtures/context-worker.cjs");
      let reply = "";
      worker.on("online", () => {
        console.log("worker online store:", storage.getStore());
      });
      worker.on("message", (message) => {
        console.log("worker message store:", storage.getStore(), message.phase);
        if (message.phase === "ready") {
          worker.postMessage({ value: 41 });
        } else {
          reply = String(message.value);
          worker.terminate();
        }
      });
      worker.on("error", reject);
      worker.on("exit", (code) => {
        console.log("worker exit store:", storage.getStore(), code);
        resolve(reply);
      });
    }),
);

console.log("worker result:", result);
console.log("worker outside:", String(storage.getStore()));
