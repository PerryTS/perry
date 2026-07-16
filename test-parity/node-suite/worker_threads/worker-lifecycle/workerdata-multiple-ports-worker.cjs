const {
  MessagePort,
  parentPort,
  workerData,
} = require("node:worker_threads");

const first = workerData?.first;
const second = workerData?.second;

if (typeof first?.postMessage === "function") first.postMessage("first");
if (typeof second?.postMessage === "function") second.postMessage("second");

parentPort.postMessage({
  firstBrand: first instanceof MessagePort,
  secondBrand: second instanceof MessagePort,
  distinct: first !== undefined && second !== undefined && first !== second,
});

first?.close?.();
second?.close?.();
