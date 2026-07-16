const { parentPort } = require("node:worker_threads");

parentPort.once("message", ({ shared }) => {
  const view = new Uint8Array(shared);
  const before = Array.from(view).join(",");
  view[1] = 8;
  parentPort.postMessage({
    brand: shared instanceof SharedArrayBuffer,
    before,
    after: Array.from(view).join(","),
  });
});
