// A natural exit waits for the worker, and its message is delivered.
import { Worker } from "node:worker_threads";
const worker = new Worker("./exit-teardown-worker.cjs");
worker.on("message", (message: string) => console.log("from worker:", message));
worker.on("exit", (code: number) => console.log("worker exit", code));
