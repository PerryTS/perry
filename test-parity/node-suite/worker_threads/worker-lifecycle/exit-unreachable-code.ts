import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const shared = new SharedArrayBuffer(4);
const view = new Int32Array(shared);
const events: string[] = [];
const worker = new Worker("./exit-unreachable-worker.cjs", {
  workerData: shared,
});

worker.on("message", (value) => events.push(`message:${value}`));
worker.on("error", (error: any) => events.push(`error:${error?.name}`));
worker.on("exit", (code) => {
  console.log("events:", events.join(",") || "none");
  console.log("exit:", code, Atomics.load(view, 0));
});
