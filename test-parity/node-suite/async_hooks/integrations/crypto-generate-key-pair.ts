import { AsyncLocalStorage } from "node:async_hooks";
import { generateKeyPair } from "node:crypto";

const storage = new AsyncLocalStorage<string>();
const result = await storage.run(
  "generate-key-pair",
  () =>
    new Promise<string>((resolve, reject) => {
      const returned = generateKeyPair(
        "rsa",
        { modulusLength: 512 },
        (error, publicKey, privateKey) => {
          console.log("generateKeyPair store:", storage.getStore());
          if (error) return reject(error);
          resolve(`${publicKey.type}:${privateKey.type}`);
        },
      );
      console.log("generateKeyPair return undefined:", returned === undefined);
    }),
);
console.log("generateKeyPair result:", result);
console.log("generateKeyPair outside:", String(storage.getStore()));
