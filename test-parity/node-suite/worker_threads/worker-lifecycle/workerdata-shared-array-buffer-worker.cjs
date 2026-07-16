const { parentPort, workerData } = require("node:worker_threads");

const view = new Uint8Array(workerData.shared);
const before = Array.from(view).join(",");
view[0] = 9;
parentPort.postMessage({
  brand: workerData.shared instanceof SharedArrayBuffer,
  before,
  after: Array.from(view).join(","),
});
