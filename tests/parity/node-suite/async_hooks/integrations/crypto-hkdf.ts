import { AsyncLocalStorage } from "node:async_hooks";
import { hkdf } from "node:crypto";

const storage = new AsyncLocalStorage<string>();
const length = await storage.run(
  "hkdf",
  () =>
    new Promise<number>((resolve, reject) => {
      const returned = hkdf(
        "sha256",
        "key",
        "salt",
        "info",
        16,
        (error, derived) => {
          console.log("hkdf store:", storage.getStore());
          if (error) return reject(error);
          resolve(derived.byteLength);
        },
      );
      console.log("hkdf return undefined:", returned === undefined);
    }),
);
console.log("hkdf length:", length);
console.log("hkdf outside:", String(storage.getStore()));
