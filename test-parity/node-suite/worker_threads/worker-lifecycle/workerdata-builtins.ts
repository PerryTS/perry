import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./builtin-data-worker.cjs", {
  workerData: {
    date: new Date("2021-02-03T04:05:06.000Z"),
    map: new Map([["key", 17]]),
    regexp: /data/i,
    bigint: 12345678901234567890n,
  },
});

worker.on("message", (message: any) => {
  console.log(
    "brands:",
    message?.date,
    message?.map,
    message?.regexp,
    message?.bigintType,
  );
  console.log(
    "values:",
    message?.dateValue,
    message?.mapValue,
    message?.regexpValue,
    message?.bigintValue,
  );
});
worker.on("exit", (code) => console.log("exit:", code));
