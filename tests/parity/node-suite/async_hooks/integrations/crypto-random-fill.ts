import { randomFill } from "node:crypto";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const target = new Uint8Array(12);

const result = await storage.run(
  "random-fill",
  () =>
    new Promise<Uint8Array>((resolve, reject) => {
      randomFill(target, 2, 6, (error, value) => {
        console.log("randomFill store:", storage.getStore());
        if (error) return reject(error);
        resolve(value);
      });
    }),
);

console.log("randomFill identity:", result === target);
console.log("randomFill length:", result.length);
console.log("randomFill outside:", String(storage.getStore()));
