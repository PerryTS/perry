const { parentPort, workerData } = require("node:worker_threads");

const view = new Int32Array(workerData);
process.exit(4);
view[0] = 99;
parentPort.postMessage("unreachable");
