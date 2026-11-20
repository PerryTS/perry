// EventEmitter ordering/once/removal contracts on the cluster singleton.
import cluster from "node:cluster";

const order: string[] = [];
const regular = () => order.push("on");
cluster.on("probe", regular);
cluster.prependListener("probe", () => order.push("prepend"));
cluster.once("probe", () => order.push("once"));
cluster.prependOnceListener("probe", () => order.push("prepend-once"));
console.log("first:", cluster.emit("probe"), order.join(","));
order.length = 0;
console.log("second:", cluster.emit("probe"), order.join(","));
console.log("count:", cluster.listenerCount("probe"));
console.log("off returns self:", cluster.off("probe", regular) === cluster);
console.log("after off:", cluster.listenerCount("probe"));
cluster.removeAllListeners("probe");
console.log(
  "after all:",
  cluster.emit("probe"),
  cluster.listenerCount("probe"),
);
