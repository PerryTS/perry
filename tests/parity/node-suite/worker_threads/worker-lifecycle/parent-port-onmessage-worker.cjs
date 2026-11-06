const { parentPort } = require("node:worker_threads");

parentPort.onmessage = (event) => {
  parentPort.postMessage({
    type: event.type,
    data: event.data * 2,
    target: event.target === parentPort,
    source: event.source === null,
  });
  parentPort.postMessage(undefined);
  parentPort.postMessage(null);
  parentPort.postMessage(event.ports.length);
};
