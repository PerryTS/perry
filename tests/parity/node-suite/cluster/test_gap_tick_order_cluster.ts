// cluster's `setup` event runs from the tick queue: before a setImmediate the
// program scheduled earlier in the same turn, as in node.
import cluster from "node:cluster";
if (cluster.isPrimary) {
  setImmediate(() => console.log("immediate"));
  cluster.on("setup", () => console.log("setup"));
  cluster.setupPrimary({});
}
