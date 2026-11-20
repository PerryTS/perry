const {
  getEnvironmentData,
  isMainThread,
  parentPort,
  threadName,
  workerData,
} = require("node:worker_threads");

const environment = getEnvironmentData("suite-environment");
const data = workerData || {};
parentPort.postMessage({
  isMainThread,
  threadName,
  buffer: data.buffer instanceof ArrayBuffer,
  view: data.view instanceof Uint8Array,
  sharedBacking:
    data.view instanceof Uint8Array &&
    data.buffer instanceof ArrayBuffer &&
    data.view.buffer === data.buffer,
  values: typeof data.view?.join === "function"
    ? data.view.join(",")
    : "not-typed",
  nested: data.nested?.value,
  environment: `${environment?.label}:${environment?.version}`,
});
