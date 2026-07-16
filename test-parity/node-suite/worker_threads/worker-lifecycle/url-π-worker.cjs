const path = require("node:path");
const { parentPort, workerData } = require("node:worker_threads");

parentPort.postMessage(`${workerData}:${path.basename(__filename)}`);
