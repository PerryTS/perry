const { getEnvironmentData, parentPort } = require("node:worker_threads");

parentPort.postMessage(getEnvironmentData("nested-key")?.level ?? "missing");
