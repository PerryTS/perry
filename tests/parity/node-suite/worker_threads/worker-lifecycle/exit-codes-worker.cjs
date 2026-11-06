const { parentPort, workerData } = require("node:worker_threads");

if (workerData === "explicit") {
  process.exit(5);
} else {
  process.exitCode = 7;
  parentPort.postMessage("set");
}
