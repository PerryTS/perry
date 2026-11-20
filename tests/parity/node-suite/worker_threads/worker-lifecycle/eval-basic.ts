import { Worker } from "node:worker_threads";

const events: string[] = [];
const worker = new Worker(
  `
    const { parentPort, workerData } = require("node:worker_threads");
    const path = require("node:path");
    parentPort.postMessage({
      sum: workerData.left + workerData.right,
      basename: path.basename("/tmp/value.txt"),
    });
  `,
  {
    eval: true,
    workerData: { left: 2, right: 3 },
    name: "eval-parity",
  },
);

worker.on("online", () => events.push("online"));
worker.on("message", (message) => {
  events.push(`message:${message.sum}:${message.basename}`);
});
worker.on("error", (error: any) => {
  events.push(`error:${error?.name}`);
});
worker.on("exit", (code) => {
  events.push(`exit:${code}`);
  console.log("events:", events.join(","));
});
