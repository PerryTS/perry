// The callback of an internal runtime completion runs in the AsyncLocalStorage
// context of the operation that started it (als.run -> operation -> callback).
import { AsyncLocalStorage } from "node:async_hooks";
const als = new AsyncLocalStorage<string>();

import cluster from "node:cluster";
if (cluster.isPrimary) {
  als.run("setup", () => {
    cluster.on("setup", () => console.log("setup event: " + als.getStore()));
    cluster.setupPrimary({});
  });
  const w = cluster.fork();
  w.on("message", (m) => {
    console.log("worker said " + m);
    als.run("disconnect", () => cluster.disconnect(() => console.log("disconnect callback: " + als.getStore())));
  });
} else {
  process.send!("hi");
}
