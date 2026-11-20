const {
  getEnvironmentData,
  parentPort,
} = require("node:worker_threads");

const graph = getEnvironmentData("graph-snapshot");
parentPort.postMessage({
  cycle: graph !== undefined && graph.self === graph,
  alias: graph?.left !== undefined && graph.left === graph.right,
  value: graph?.left?.value ?? "missing",
});
