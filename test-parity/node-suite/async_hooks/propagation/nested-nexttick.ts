import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const stores = await storage.run(
  "nested-tick",
  () =>
    new Promise<string[]>((resolve) => {
      process.nextTick(() => {
        const seen = [String(storage.getStore())];
        process.nextTick(() => {
          seen.push(String(storage.getStore()));
          resolve(seen);
        });
      });
    }),
);
console.log("nested nextTick stores:", stores.join(","));
console.log("nested nextTick outside:", String(storage.getStore()));
