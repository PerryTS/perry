const { parentPort } = require("node:worker_threads");
import("./entry-retention-helper.mjs").then((module) => {
  parentPort.postMessage(module.value);
});
