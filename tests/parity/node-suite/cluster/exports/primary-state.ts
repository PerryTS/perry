// Node v26.5.0 primary.js constants and initial mutable registries.
import cluster from "node:cluster";

console.log("roles:", cluster.isPrimary, cluster.isMaster, cluster.isWorker);
console.log("constants:", cluster.SCHED_NONE, cluster.SCHED_RR);
console.log(
  "policy is valid:",
  [cluster.SCHED_NONE, cluster.SCHED_RR].includes(cluster.schedulingPolicy),
);
console.log("settings keys:", Object.keys(cluster.settings).sort().join(","));
console.log(
  "workers keys:",
  Object.keys(cluster.workers ?? {}).sort().join(","),
);
console.log("worker absent:", (cluster as any).worker === undefined);
