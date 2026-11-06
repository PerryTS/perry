const { parentPort } = require("node:worker_threads");

process.on("workerMessage", () => {});
parentPort.on("message", () => {});
parentPort.postMessage({ phase: "ready" });

const start = Date.now();
while (Date.now() - start < 100) {}
