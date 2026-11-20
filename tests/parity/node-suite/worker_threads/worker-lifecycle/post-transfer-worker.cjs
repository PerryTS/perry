const { parentPort } = require("node:worker_threads");

parentPort.on("message", (message) => {
  const view = message?.view;
  parentPort.postMessage({
    brand: view instanceof Uint8Array,
    length: view?.length,
    values: typeof view?.join === "function" ? view.join(",") : "not-typed",
  });
});
