import { Worker, postMessageToThread } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/direct-message");

const worker = new Worker("./timeout-worker.cjs");

worker.on("message", async (message: any) => {
  if (message.phase !== "ready") {
    return;
  }

  try {
    await postMessageToThread(worker.threadId, "slow", undefined, 1);
    console.log("timeout: resolved");
  } catch (err: any) {
    console.log("timeout:", err.name, err.code, err.message);
  }

  worker.terminate().then((code) => {
    console.log("terminate:", code);
  });
});
