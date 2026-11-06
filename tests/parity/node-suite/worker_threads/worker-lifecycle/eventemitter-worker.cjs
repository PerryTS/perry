const { parentPort } = require("node:worker_threads");

parentPort.postMessage("first");
parentPort.postMessage("second");
