const { parentPort, workerData } = require("node:worker_threads");
const left = workerData?.left;
const right = workerData?.right;
const view = workerData?.view;
const buffer = workerData?.buffer;

parentPort.postMessage({
  alias: left !== undefined && left === right,
  viewBrand: view instanceof Uint16Array,
  bufferBrand: buffer instanceof ArrayBuffer,
  backing: view !== undefined && view.buffer === buffer,
  value: view?.[0] ?? "missing",
  lengths: [buffer?.byteLength ?? "missing", view?.byteLength ?? "missing"],
});
