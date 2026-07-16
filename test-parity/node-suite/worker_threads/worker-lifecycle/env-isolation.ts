import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const originalParent = process.env.PERRY_ISOLATED_PARENT;
const originalWorker = process.env.PERRY_ISOLATED_WORKER;
process.env.PERRY_ISOLATED_PARENT = "parent-value";
delete process.env.PERRY_ISOLATED_WORKER;

let observed: any;
let workerError: any;
const worker = new Worker("./env-isolation-worker.cjs");
worker.once("message", (message) => {
  observed = message;
});
worker.once("error", (error) => {
  workerError = error;
});
worker.once("exit", (code) => {
  console.log(
    "worker:",
    observed?.inherited,
    observed?.changed,
    observed?.worker,
    workerError?.name ?? "no-error",
  );
  console.log(
    "parent:",
    process.env.PERRY_ISOLATED_PARENT,
    process.env.PERRY_ISOLATED_WORKER,
    code,
  );

  if (originalParent === undefined) delete process.env.PERRY_ISOLATED_PARENT;
  else process.env.PERRY_ISOLATED_PARENT = originalParent;
  if (originalWorker === undefined) delete process.env.PERRY_ISOLATED_WORKER;
  else process.env.PERRY_ISOLATED_WORKER = originalWorker;
});
