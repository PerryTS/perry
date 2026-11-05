// Node v26.5.0 test-cluster-disconnect-with-no-workers.js: completion is
// deferred even when there is nothing to disconnect.
import cluster from "node:cluster";

let sync = true;
const returned = cluster.disconnect(() => console.log("callback sync:", sync));
console.log("returns undefined:", returned === undefined);
sync = false;
