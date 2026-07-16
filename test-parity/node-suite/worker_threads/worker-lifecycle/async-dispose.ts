import { Worker } from "node:worker_threads";

const worker = new Worker(
  `
  const { parentPort } = require("node:worker_threads");
  parentPort.on("message", () => {});
  parentPort.postMessage("ready");
`,
  { eval: true },
);

worker.once("message", async () => {
  const key = (Symbol as any).asyncDispose;
  const dispose = (worker as any)[key];
  console.log("method:", typeof dispose);

  if (typeof dispose !== "function") {
    console.log("fallback terminate:", await worker.terminate());
    return;
  }

  let exitCode: number | "pending" = "pending";
  worker.once("exit", (code) => {
    exitCode = code;
  });

  const result = await Reflect.apply(dispose, worker, []);
  console.log("settled:", result, exitCode, worker.threadId);
});
