const { getEnvironmentData, parentPort, workerData } = require("node:worker_threads");
const inherited = getEnvironmentData("alias");
parentPort.postMessage({ same: inherited === workerData, count: inherited.count });
