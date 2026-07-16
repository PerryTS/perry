import { postMessageToThread, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/direct-message");

const worker = new Worker("./handler-error-worker.cjs");
worker.on("message", async (message) => {
  if (message !== "ready") {
    return;
  }

  try {
    await postMessageToThread(worker.threadId, "trigger");
    console.log("direct: resolved");
  } catch (error: any) {
    console.log("direct:", error?.name, error?.code ?? "");
  }
  worker.postMessage("close");
});
worker.on(
  "error",
  (error: any) => console.log("worker error:", error?.name, error?.code ?? ""),
);
worker.on("exit", (code) => console.log("exit:", code));
