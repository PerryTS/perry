const { parentPort } = require("node:worker_threads");

process.on("workerMessage", (value, source) => {
  parentPort.postMessage({
    phase: "direct",
    text: value.text,
    count: value.count,
    nestedOk: value.nested.ok,
    source,
  });
});

parentPort.on("message", () => {});
parentPort.postMessage({ phase: "ready" });
