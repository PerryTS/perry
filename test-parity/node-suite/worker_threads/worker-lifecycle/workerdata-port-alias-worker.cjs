const {
  MessagePort,
  parentPort,
  workerData,
} = require("node:worker_threads");

const left = workerData?.left;
const right = workerData?.right;

if (typeof left?.postMessage === "function") left.postMessage("left");
if (typeof right?.postMessage === "function") right.postMessage("right");

parentPort.postMessage({
  leftBrand: left instanceof MessagePort,
  rightBrand: right instanceof MessagePort,
  alias: left !== undefined && left === right,
});

left?.close?.();
