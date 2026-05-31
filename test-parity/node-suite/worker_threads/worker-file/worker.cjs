const { isMainThread, parentPort, workerData } = require("node:worker_threads");

parentPort.postMessage({
  phase: "startup",
  main: isMainThread,
  value: `${workerData.label}:${workerData.count + 1}`,
});

parentPort.on("message", (message) => {
  parentPort.postMessage({
    phase: "reply",
    value: message.value + 1,
    nestedOk: message.nested.ok,
  });
});
