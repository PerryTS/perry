const { MessagePort, parentPort, workerData } = require("node:worker_threads");

const port = workerData?.port;
const methods = ["addListener", "off", "on", "once", "postMessage"]
  .map((name) => `${name}:${typeof port?.[name]}`)
  .join(",");

if (typeof port?.postMessage === "function") {
  port.postMessage("from-transferred-port");
}
parentPort.postMessage({
  brand: port instanceof MessagePort,
  methods,
});
try {
  port?.close?.();
} catch {}
