// Adapted from Node v26.5.0 test-cluster-worker-constructor.js.
import cluster from "node:cluster";
import { EventEmitter } from "node:events";

const empty = new cluster.Worker();
console.log(
  "empty:",
  empty.id,
  empty.state,
  empty.process,
  empty.exitedAfterDisconnect,
);
console.log(
  "empty emitter:",
  empty instanceof EventEmitter,
  empty instanceof cluster.Worker,
);

const supplied = new cluster.Worker({ id: 3, state: "online", process } as any);
console.log(
  "supplied:",
  supplied.id,
  supplied.state,
  supplied.process === process,
);
console.log(
  "methods:",
  ["send", "kill", "destroy", "disconnect", "isConnected", "isDead"].map((x) =>
    typeof (supplied as any)[x]
  ).join(","),
);
