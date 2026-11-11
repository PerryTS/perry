import { Worker } from "node:worker_threads";

let observedThreadId: number | undefined;
process.once("worker", (created: any) => {
  observedThreadId = created.threadId;
});

const worker = new Worker("", { eval: true });
const initialThreadId = worker.threadId;
worker.on("exit", (code) => {
  console.log("event observed:", observedThreadId !== undefined);
  console.log("thread match:", observedThreadId === initialThreadId);
  console.log("exit:", code);
});
