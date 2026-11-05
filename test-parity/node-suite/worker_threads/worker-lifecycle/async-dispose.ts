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

  const first = Reflect.apply(dispose, worker, []);
  const second = Reflect.apply(dispose, worker, []);
  const termination = worker.terminate();
  console.log("distinct:", first !== second);
  const result = await first;
  console.log("settled:", result, exitCode, worker.threadId);
  console.log("repeated:", await second, await termination, await Reflect.apply(dispose, worker, []));
  const natural = new Worker(`const {parentPort} = require("node:worker_threads");
    parentPort.once("message", () => parentPort.close()); parentPort.postMessage("ready");`, { eval: true });
  natural.once("message", () => natural.postMessage("finish"));
  natural.once("exit", async (code) => {
    console.log("natural:", code, await Reflect.apply(dispose, natural, []), natural.threadId);
  });
});
