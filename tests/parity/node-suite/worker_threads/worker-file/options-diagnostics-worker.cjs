const {
  parentPort,
  resourceLimits,
  threadName,
  workerData,
} = require("node:worker_threads");

parentPort.postMessage({
  workerDataValue: `${workerData.label}:${workerData.count + 1}`,
  threadName,
  resourceLimits,
  envValue: process.env.PERRY_WORKER_ENV_TEST,
  pathMissing: process.env.PATH === undefined,
});

parentPort.on("message", () => {});
