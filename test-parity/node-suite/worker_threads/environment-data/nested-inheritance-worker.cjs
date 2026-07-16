const path = require("node:path");
const {
  getEnvironmentData,
  parentPort,
  setEnvironmentData,
  Worker,
  workerData,
} = require("node:worker_threads");

const inherited = getEnvironmentData("nested-key");
const first = inherited?.level ?? "missing";
setEnvironmentData("nested-key", { level: workerData.level });

const nested = new Worker(path.join(__dirname, "nested-leaf-worker.cjs"));
nested.on("message", (second) => parentPort.postMessage({ first, second }));
