const { parentPort, workerData } = require("node:worker_threads");

parentPort.postMessage({
  label: workerData,
  before: process.env.PERRY_PARENT_BEFORE ?? null,
  after: process.env.PERRY_PARENT_AFTER ?? null,
  manual: process.env.PERRY_MANUAL ?? null,
  boolean: process.env.PERRY_BOOLEAN ?? null,
});
