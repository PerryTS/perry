import { AsyncLocalStorage } from "node:async_hooks";
import { generateKey } from "node:crypto";

const storage = new AsyncLocalStorage<string>();
const result = await storage.run(
  "generate-key",
  () =>
    new Promise<string>((resolve, reject) => {
      const returned = generateKey("hmac", { length: 64 }, (error, key) => {
        console.log("generateKey store:", storage.getStore());
        if (error) return reject(error);
        resolve(`${key.type}:${key.symmetricKeySize}`);
      });
      console.log("generateKey return undefined:", returned === undefined);
    }),
);
console.log("generateKey result:", result);
console.log("generateKey outside:", String(storage.getStore()));
