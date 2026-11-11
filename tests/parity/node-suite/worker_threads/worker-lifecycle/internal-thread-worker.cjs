const workerThreads = require("node:worker_threads");

workerThreads.parentPort.postMessage({
  named: workerThreads.isInternalThread,
  namespace: require("node:worker_threads").isInternalThread,
  main: workerThreads.isMainThread,
});
