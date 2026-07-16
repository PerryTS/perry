import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./postmessage-errors-worker.cjs");
worker.on("online", () => {
  try {
    (worker.postMessage as any)();
    console.log("post: ok");
  } catch (error: any) {
    console.log("post:", error?.name, error?.code ?? "");
  }
  worker.postMessage("finish");
});
worker.on("message", (message) => {
  console.log("message:", message);
  worker.terminate().then((code) => console.log("terminate:", code));
});
worker.on("exit", (code) => console.log("exit:", code));
