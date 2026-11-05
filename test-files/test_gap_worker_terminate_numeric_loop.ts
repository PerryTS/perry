// Worker-capable images must retain interrupts in call-free numeric loops.
import { Worker } from "node:worker_threads";

const worker = new Worker(new URL("./_helpers/worker_terminate_numeric_loop.ts", import.meta.url));
worker.on("message", () => {
  setTimeout(() => {
    worker.terminate().then((code: number) => console.log("terminated", code));
  }, 50);
});
