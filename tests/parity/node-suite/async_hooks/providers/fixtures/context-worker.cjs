const { parentPort } = require("node:worker_threads");

parentPort.postMessage({ phase: "ready" });
parentPort.on("message", (message) => {
  parentPort.postMessage({ phase: "reply", value: message.value + 1 });
});
