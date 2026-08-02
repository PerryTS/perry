// EventEmitter listener arguments are validated synchronously on the cluster
// singleton for both registration and removal methods.
import cluster from "node:cluster";

for (
  const method of [
    "on",
    "once",
    "prependListener",
    "prependOnceListener",
    "removeListener",
    "off",
  ] as const
) {
  try {
    cluster[method]("probe", 42 as never);
  } catch (error: any) {
    console.log(method, error.name, error.code);
  }
}

const duplicate = () => {};
cluster.on("count", duplicate);
cluster.on("count", duplicate);
cluster.on("count", () => {});
console.log(
  "filtered count",
  cluster.listenerCount("count"),
  cluster.listenerCount("count", duplicate),
  cluster.listenerCount("count", 42 as never),
);

cluster.on(null as never, () => {});
cluster.on(undefined as never, () => {});
cluster.removeAllListeners(null as never);
console.log(
  "explicit event",
  cluster.listenerCount("null"),
  cluster.listenerCount("undefined"),
  cluster.listenerCount("count"),
);
cluster.removeAllListeners();
console.log("all events", cluster.eventNames().length);

const regular = () => {};
cluster.on("surface", regular);
cluster.once("surface", () => {});
console.log(
  "listener arrays",
  cluster.listeners("surface").length,
  cluster.rawListeners("surface").length,
  cluster.listeners("surface")[0] === regular,
);
console.log("max default", cluster.getMaxListeners());
console.log(
  "max set",
  cluster.setMaxListeners(17) === cluster,
  cluster.getMaxListeners(),
);
