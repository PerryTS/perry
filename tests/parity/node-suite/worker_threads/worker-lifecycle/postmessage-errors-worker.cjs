const { parentPort } = require("node:worker_threads");

parentPort.once("message", (message) => {
  if (message === "finish") {
    parentPort.postMessage("finished");
  }
});
