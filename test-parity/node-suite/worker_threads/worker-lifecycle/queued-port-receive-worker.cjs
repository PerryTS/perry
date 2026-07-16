const {
  receiveMessageOnPort,
  parentPort,
  workerData,
} = require("node:worker_threads");

const packet = receiveMessageOnPort(workerData.port);
const empty = receiveMessageOnPort(workerData.port);
parentPort.postMessage({
  text: packet?.message?.text ?? "missing",
  count: packet?.message?.count ?? "missing",
  empty: empty === undefined,
});
try {
  workerData.port.close();
} catch (error) {
  parentPort.postMessage({ closeError: error?.name ?? "Error" });
}
