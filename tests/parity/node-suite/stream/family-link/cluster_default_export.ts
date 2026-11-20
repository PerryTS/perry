// The cluster default export read from a primary: an EventEmitter whose
// prototype is EventEmitter.prototype, with on / emit / listenerCount.
import cluster from "node:cluster";
import { EventEmitter } from "node:events";

console.log("isPrimary", cluster.isPrimary);
console.log("proto", Object.getPrototypeOf(cluster) === EventEmitter.prototype, cluster instanceof EventEmitter);
let seen = 0;
const onX = (n: number) => {
  seen += n;
};
console.log("on returns cluster", cluster.on("x", onX) === cluster);
console.log("emit", cluster.emit("x", 2), cluster.emit("y"));
console.log("count", cluster.listenerCount("x"), seen);
cluster.removeListener("x", onX);
console.log("after remove", cluster.listenerCount("x"));
