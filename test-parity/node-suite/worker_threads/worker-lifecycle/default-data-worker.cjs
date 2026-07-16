const { isMainThread, parentPort, workerData } = require("node:worker_threads");

parentPort.postMessage({
  isMainThread,
  type: typeof workerData,
  isNull: workerData === null,
  value: workerData?.value,
});
