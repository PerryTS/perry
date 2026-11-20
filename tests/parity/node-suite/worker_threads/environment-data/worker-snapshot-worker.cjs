const { getEnvironmentData, parentPort } = require("node:worker_threads");

const value = getEnvironmentData("snapshot");
if (!value || !value.nested || !value.values) {
  parentPort.postMessage({
    initialCount: "missing",
    values: "missing",
    localCount: "missing",
  });
} else {
  const initialCount = value.nested.count;
  value.nested.count = 5;

  parentPort.postMessage({
    initialCount,
    values: value.values.join(","),
    localCount: getEnvironmentData("snapshot").nested.count,
  });
}
