import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const events: string[] = [];
const worker = new Worker("./error-worker.cjs", { name: "error-worker" });

worker.on("online", () => events.push("online"));
worker.on("error", (error: any) => {
  events.push("error");
  console.log("error:", error?.name, error?.message);
});
worker.on("exit", (code) => {
  events.push("exit");
  console.log("lifecycle:", events.join(","), code);
});
