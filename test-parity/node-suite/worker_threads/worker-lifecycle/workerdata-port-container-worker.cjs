const {
  MessagePort,
  parentPort,
  workerData,
} = require("node:worker_threads");

const kind = workerData?.kind;
const container = workerData?.container;
let port;
let extractionError = "none";

try {
  if (kind === "map" && typeof container?.get === "function") {
    port = container.get("port");
  } else if (kind === "set" && typeof container?.values === "function") {
    const iterator = container.values();
    if (typeof iterator?.next === "function") port = iterator.next()?.value;
  }
} catch (error) {
  extractionError = error?.name ?? "unknown";
}

if (typeof port?.postMessage === "function") port.postMessage(kind);
parentPort.postMessage({
  kind,
  containerBrand: kind === "map"
    ? container instanceof Map
    : container instanceof Set,
  portBrand: port instanceof MessagePort,
  extractionError,
});
port?.close?.();
