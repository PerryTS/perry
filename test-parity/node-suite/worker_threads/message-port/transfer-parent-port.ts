import { Worker } from "node:worker_threads";

const worker = new Worker(new URL("./fixtures/transfer-parent-worker.ts", import.meta.url));
worker.on("message", (message) => {
  if (message.kind === "ready") {
    worker.postMessage("ping");
  } else {
    console.log("parent transfer", message.alias, message.controlDistinct, message.value);
    worker.postMessage("stop");
  }
});
worker.on("exit", (code) => console.log("exit", code));
