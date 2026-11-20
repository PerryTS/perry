// The callback of an internal runtime completion runs in the AsyncLocalStorage
// context of the operation that started it (als.run -> operation -> callback).
import { AsyncLocalStorage } from "node:async_hooks";
const als = new AsyncLocalStorage<string>();

import cluster from "node:cluster";
if (cluster.isPrimary) {
  const w = cluster.fork();
  w.on("message", (m) => { console.log(m); if (m === "sent") w.disconnect(); });
} else {
  als.run("send", () => process.send!("hello", () => process.send!("callback: " + als.getStore(), () => process.send!("sent"))));
}
