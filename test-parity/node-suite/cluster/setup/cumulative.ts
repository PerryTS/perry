// Node setupPrimary creates a fresh settings snapshot while preserving prior
// keys not overridden by later calls.
import cluster from "node:cluster";

cluster.setupPrimary({ exec: "first", args: ["a"], silent: true });
const first = cluster.settings;
cluster.setupPrimary({ args: ["b", "c"], serialization: "advanced" });
const second = cluster.settings;
console.log("replaced:", first !== second);
console.log("preserved:", second.exec, second.silent);
console.log("overridden:", JSON.stringify(second.args));
console.log("added:", second.serialization);
cluster.setupPrimary();
console.log(
  "no-option preserves:",
  cluster.settings.exec,
  JSON.stringify(cluster.settings.args),
  cluster.settings.serialization,
);
