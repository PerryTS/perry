const { parentPort } = require("node:worker_threads");

process.on("workerMessage", () => {
  throw new Error("direct-handler-boom");
});
parentPort.once("message", () => {});
parentPort.postMessage("ready");
