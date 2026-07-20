// Node exposes primary state as ordinary writable/enumerable/configurable
// own data properties, not accessors.
import cluster from "node:cluster";

for (
  const name of [
    "isPrimary",
    "isMaster",
    "isWorker",
    "schedulingPolicy",
    "settings",
    "workers",
  ]
) {
  const d = Object.getOwnPropertyDescriptor(cluster, name)!;
  console.log(
    name,
    !!d,
    "value" in d,
    d.writable,
    d.enumerable,
    d.configurable,
  );
}
