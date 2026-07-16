import { AsyncLocalStorage } from "node:async_hooks";
import { randomInt } from "node:crypto";

const storage = new AsyncLocalStorage<string>();
const result = await storage.run(
  "random-int",
  () =>
    new Promise<boolean>((resolve, reject) => {
      const returned = randomInt(10, 20, (error, value) => {
        console.log("randomInt store:", storage.getStore());
        if (error) return reject(error);
        resolve(value >= 10 && value < 20);
      });
      console.log("randomInt return undefined:", returned === undefined);
    }),
);
console.log("randomInt in range:", result);
console.log("randomInt outside:", String(storage.getStore()));
