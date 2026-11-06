const { parentPort } = require("node:worker_threads");
parentPort.on("message", (message) => {
  parentPort.postMessage(message === undefined ? "undefined" : message);
  if (message === "finish") parentPort.close();
});
