const { parentPort, resourceLimits } = require("node:worker_threads");

parentPort.postMessage(resourceLimits);
