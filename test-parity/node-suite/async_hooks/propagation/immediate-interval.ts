import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

await storage.run(
  "immediate",
  () =>
    new Promise<void>((resolve) => {
      setImmediate(() => {
        console.log("immediate store:", storage.getStore());
        resolve();
      });
    }),
);

await storage.run(
  "interval",
  () =>
    new Promise<void>((resolve) => {
      let count = 0;
      const interval = setInterval(() => {
        count++;
        console.log("interval store:", count, storage.getStore());
        if (count === 2) {
          clearInterval(interval);
          resolve();
        }
      }, 1);
    }),
);

console.log("timer variants outside:", String(storage.getStore()));
