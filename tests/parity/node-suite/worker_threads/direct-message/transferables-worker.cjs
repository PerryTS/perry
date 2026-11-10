const {
  MessagePort,
  parentPort,
} = require("node:worker_threads");

process.on("workerMessage", (value, source) => {
  const port = value?.port;
  const buffer = value?.buffer;
  port?.postMessage({
    portBrand: port instanceof MessagePort,
    bufferBrand: buffer instanceof ArrayBuffer,
    values: buffer instanceof ArrayBuffer
      ? Array.from(new Uint8Array(buffer)).join(",")
      : "missing",
    source,
  });
  port?.close();
});

parentPort.on("message", () => {});
parentPort.postMessage("ready");
