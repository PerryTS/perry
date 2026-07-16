const { parentPort } = require("node:worker_threads");

const inherited = process.env.PERRY_ISOLATED_PARENT;
process.env.PERRY_ISOLATED_PARENT = "worker-change";
process.env.PERRY_ISOLATED_WORKER = "worker-value";

parentPort.postMessage({
  inherited,
  changed: process.env.PERRY_ISOLATED_PARENT,
  worker: process.env.PERRY_ISOLATED_WORKER,
});
