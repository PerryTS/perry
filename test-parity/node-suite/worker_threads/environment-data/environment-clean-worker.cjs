const { getEnvironmentData, parentPort } = require("node:worker_threads");

parentPort.postMessage(getEnvironmentData("uncloneable") === undefined);
