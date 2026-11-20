const { parentPort } = require("node:worker_threads");

Object.assign(globalThis, {
  postMessage(message) {
    parentPort.postMessage(message);
  },
});

parentPort.once("message", (message) => {
  globalThis.postMessage(`echo:${message}`);
});
parentPort.postMessage("ready");
