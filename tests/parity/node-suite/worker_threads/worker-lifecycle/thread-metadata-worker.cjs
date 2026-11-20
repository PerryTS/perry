const { parentPort, threadId, threadName } = require("node:worker_threads");

parentPort.postMessage({ threadId, threadName });
