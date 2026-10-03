const { parentPort } = require("node:worker_threads");

class WorkerNode {
  constructor() {
    this.kind = 1;
  }
}
const nodes = [];
for (let n = 0; n < 12; n++) {
  const node = new WorkerNode();
  for (let k = 0; k < n; k++) node["extra" + k] = k;
  nodes.push(node);
}
function read(node) {
  const value = node.optional;
  return value === undefined ? 0 : value;
}
function sumReads() {
  let sum = 0;
  for (let i = 0; i < 1200; i++) sum += read(nodes[i % nodes.length]);
  return sum;
}
parentPort.on("message", (message) => {
  if (message !== "go") return;
  let churn = [];
  for (let i = 0; i < 20000; i++) churn.push({ i });
  global.gc();
  churn = [];
  parentPort.postMessage(sumReads());
});
parentPort.postMessage("ready");
