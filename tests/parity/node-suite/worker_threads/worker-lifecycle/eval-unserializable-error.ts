import { Worker } from "node:worker_threads";

const worker = new Worker(
  `
    const { MessageChannel } = require("node:worker_threads");
    throw new MessageChannel().port1;
  `,
  { eval: true },
);

let summary = "missing";
worker.on("error", (error: any) => {
  summary = [
    typeof error,
    error instanceof Error,
    Object.getPrototypeOf(error) === null,
    Object.prototype.toString.call(error),
    typeof error?.name,
    typeof error?.message,
  ].join(":");
});
worker.on("exit", (code) => {
  console.log("error:", summary);
  console.log("exit:", code);
});
