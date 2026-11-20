import { Worker } from "node:worker_threads";

const originalEmit = process.emit;
let worker: Worker;
let synchronous = true;

worker = new Worker("", { eval: true });
(process as any).emit = function (event: string, value: any) {
  if (event === "worker") {
    process.emit = originalEmit;
    console.log("late emit:", !synchronous, value === worker);
    return true;
  }
  return originalEmit.apply(this, arguments as any);
};
synchronous = false;

worker.on("exit", (code) => console.log("exit:", code));
