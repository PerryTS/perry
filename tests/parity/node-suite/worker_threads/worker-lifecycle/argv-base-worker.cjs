const { parentPort } = require("node:worker_threads");

parentPort.postMessage({
  exec: process.argv[0] === process.execPath,
  script: process.argv[1]?.endsWith("argv-base-worker.cjs"),
  tail: process.argv.slice(2),
});
