const { parentPort, workerData } = require("node:worker_threads");
parentPort.postMessage({
  cycle: workerData.self === workerData,
  nested: workerData.child.parent === workerData,
  value: workerData.child.value,
});
