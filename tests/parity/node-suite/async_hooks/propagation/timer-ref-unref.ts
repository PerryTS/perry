import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
await storage.run(
  "ref-unref",
  () =>
    new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        console.log("ref/unref timer store:", storage.getStore());
        resolve();
      }, 5);
      console.log("timer initially refed:", timer.hasRef());
      timer.unref();
      console.log("timer after unref:", timer.hasRef());
      timer.ref();
      console.log("timer after ref:", timer.hasRef());
    }),
);
console.log("ref/unref outside:", String(storage.getStore()));
