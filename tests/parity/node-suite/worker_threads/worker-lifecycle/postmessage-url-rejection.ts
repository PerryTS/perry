import { Worker } from "node:worker_threads";

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const worker = new Worker(
  `
    const { parentPort } = require("node:worker_threads");
    parentPort.on("message", (message) => parentPort.postMessage(message));
  `,
  { eval: true },
);
worker.on("online", () => {
  const url = new URL("https://example.org/path?q=value");
  console.log("worker:", outcome(() => worker.postMessage(url)));
  worker.postMessage("still-alive");
});
worker.on("message", (message) => {
  console.log("worker message:", message);
  worker.terminate().then((code) => console.log("terminate:", code));
});
worker.on("exit", (code) => console.log("exit:", code));
