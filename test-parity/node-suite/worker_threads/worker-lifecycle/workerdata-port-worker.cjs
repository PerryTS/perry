const {
  MessagePort,
  parentPort,
  workerData,
} = require("node:worker_threads");

const port = workerData?.port;
if (typeof port?.postMessage === "function") {
  port.postMessage("from-worker");
}
parentPort.postMessage({
  brand: port instanceof MessagePort,
  postMessage: typeof port?.postMessage,
});
