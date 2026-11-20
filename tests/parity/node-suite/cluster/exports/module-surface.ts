// Node v26.5.0 lib/internal/cluster/primary.js and Deno's cluster_test.ts:
// primary exports and their aliases are stable singleton values.
import cluster from "node:cluster";
import * as namespace from "node:cluster";

const names = [
  "Worker",
  "disconnect",
  "fork",
  "setupMaster",
  "setupPrimary",
  "SCHED_NONE",
  "SCHED_RR",
  "isMaster",
  "isPrimary",
  "isWorker",
  "schedulingPolicy",
  "settings",
  "workers",
];

for (const name of names) {
  const value = (cluster as any)[name];
  console.log(name, typeof value, value === (namespace as any)[name]);
}
console.log("setup alias:", cluster.setupPrimary === cluster.setupMaster);
console.log("default singleton:", namespace.default === cluster);
