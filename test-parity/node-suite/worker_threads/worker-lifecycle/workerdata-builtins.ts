import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./builtin-data-worker.cjs", {
  workerData: {
    date: new Date("2021-02-03T04:05:06.000Z"),
    map: new Map([["key", 17]]),
    set: new Set([2, 3]),
    regexp: /data/i,
    bigint: 12345678901234567890n,
    error: new TypeError("worker-data-error"),
  },
});

worker.on("message", (message: any) => {
  console.log(
    "brands:",
    message?.date,
    message?.map,
    message?.set,
    message?.regexp,
    message?.bigintType,
    message?.error,
  );
  console.log(
    "values:",
    message?.dateValue,
    message?.mapValue,
    message?.setValue,
    message?.regexpValue,
    message?.bigintValue,
    message?.errorValue,
  );
});
worker.on("exit", (code) => console.log("exit:", code));
