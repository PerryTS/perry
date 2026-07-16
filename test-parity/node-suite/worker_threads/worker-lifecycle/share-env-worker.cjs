const { parentPort } = require("node:worker_threads");

parentPort.postMessage({
  phase: "ready",
  parent: process.env.PERRY_SHARED_PARENT,
});
parentPort.once("message", () => {
  process.env.PERRY_SHARED_WORKER = "worker-value";
  parentPort.postMessage({
    phase: "checked",
    parent: process.env.PERRY_SHARED_PARENT,
  });
});
