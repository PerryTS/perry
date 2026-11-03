// The callback of an internal runtime completion runs in the AsyncLocalStorage
// context of the operation that started it (als.run -> operation -> callback).
import { AsyncLocalStorage } from "node:async_hooks";
const als = new AsyncLocalStorage<string>();

import { MessageChannel } from "node:worker_threads";
als.run("port", () => {
  const { port1, port2 } = new MessageChannel();
  port2.on("message", (m) => { console.log("message " + m + ": " + als.getStore()); port1.close(); });
  port1.postMessage(1);
});
