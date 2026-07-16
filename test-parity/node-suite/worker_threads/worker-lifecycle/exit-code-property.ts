import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./exit-codes-worker.cjs", {
  workerData: "exitCode",
});
const events: string[] = [];
worker.on("message", (message) => events.push(`message:${message}`));
worker.on("error", (error: any) => events.push(`error:${error?.name}`));
worker.on("exit", (code) => {
  events.push(`exit:${code}`);
  console.log("exitCode:", events.join(","));
});
