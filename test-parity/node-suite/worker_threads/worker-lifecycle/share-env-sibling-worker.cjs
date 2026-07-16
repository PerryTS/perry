const { parentPort } = require("node:worker_threads");

parentPort.on("message", (command) => {
  if (command === "mutate") {
    process.env.PERRY_SHARED_SIBLING = "worker-value";
    delete process.env.PERRY_SHARED_DELETE;
    parentPort.postMessage({
      phase: "mutated",
      value: process.env.PERRY_SHARED_SIBLING,
      deleted: process.env.PERRY_SHARED_DELETE === undefined,
    });
    return;
  }

  parentPort.postMessage({
    phase: "inspected",
    value: process.env.PERRY_SHARED_SIBLING,
    deleted: process.env.PERRY_SHARED_DELETE === undefined,
    enumerated: Object.keys(process.env).includes("PERRY_SHARED_SIBLING"),
  });
});
parentPort.postMessage({ phase: "ready" });
