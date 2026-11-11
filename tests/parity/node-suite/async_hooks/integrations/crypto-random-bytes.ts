import { randomBytes } from "node:crypto";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const length = await storage.run(
  "random-bytes",
  () =>
    new Promise<number>((resolve, reject) => {
      randomBytes(16, (error, bytes) => {
        console.log("randomBytes store:", storage.getStore());
        if (error) return reject(error);
        resolve(bytes.length);
      });
    }),
);

console.log("randomBytes length:", length);
console.log("randomBytes outside:", String(storage.getStore()));
