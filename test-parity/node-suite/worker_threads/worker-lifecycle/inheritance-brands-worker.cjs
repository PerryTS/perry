const { MessagePort, parentPort } = require("node:worker_threads");

parentPort.postMessage({
  eventTarget: parentPort instanceof EventTarget,
  messagePort: parentPort instanceof MessagePort,
});
