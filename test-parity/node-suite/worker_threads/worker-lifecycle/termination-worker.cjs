const { parentPort } = require("node:worker_threads");

parentPort.on("message", () => {});
parentPort.postMessage("ready");
