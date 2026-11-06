import { AsyncLocalStorage } from "node:async_hooks";
import { runInNewContext } from "node:vm";

const storage = new AsyncLocalStorage<string>();
const observed = await storage.run(
  "vm-context",
  () =>
    new Promise<string>((resolve) => {
      runInNewContext(
        "setImmediate(() => resolve(String(storage.getStore())))",
        {
          resolve,
          setImmediate,
          storage,
          String,
        },
      );
    }),
);
console.log("vm immediate store:", observed);
console.log("vm outside:", String(storage.getStore()));
