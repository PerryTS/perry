import { resolve4 } from "node:dns";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const completed = await storage.run(
  "dns-resolve4",
  () =>
    new Promise<string>((resolve) => {
      resolve4("localhost", () => {
        console.log("dns resolve4 store:", storage.getStore());
        console.log("dns resolve4 completed:", true);
        resolve("done");
      });
    }),
);

console.log("dns resolve4 result:", completed);
console.log("dns resolve4 outside:", String(storage.getStore()));
