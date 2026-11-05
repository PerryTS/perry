// Node v26.5.0 Worker is an EventEmitter subclass and its seven public
// prototype entries are supplied by internal/cluster/worker.js + primary.js.
import cluster from "node:cluster";
import { EventEmitter } from "node:events";

console.log("cluster EventEmitter:", cluster instanceof EventEmitter);
console.log(
  "cluster prototype:",
  Object.getPrototypeOf(cluster) === EventEmitter.prototype,
);
console.log(
  "Worker typeof/length:",
  typeof cluster.Worker,
  cluster.Worker.length,
);
console.log(
  "Worker extends EventEmitter:",
  Object.getPrototypeOf(cluster.Worker.prototype) === EventEmitter.prototype,
);
console.log(
  "Worker methods:",
  Object.getOwnPropertyNames(cluster.Worker.prototype).sort().join(","),
);
