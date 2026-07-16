import { pbkdf2, scrypt } from "node:crypto";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const pbkdf2Length = await storage.run(
  "pbkdf2",
  () =>
    new Promise<number>((resolve, reject) => {
      pbkdf2("password", "salt", 2, 16, "sha256", (error, key) => {
        console.log("pbkdf2 store:", storage.getStore());
        if (error) return reject(error);
        resolve(key.length);
      });
    }),
);
console.log("pbkdf2 length:", pbkdf2Length);

const scryptLength = await storage.run(
  "scrypt",
  () =>
    new Promise<number>((resolve, reject) => {
      scrypt("password", "salt", 16, (error, key) => {
        console.log("scrypt store:", storage.getStore());
        if (error) return reject(error);
        resolve(key.length);
      });
    }),
);
console.log("scrypt length:", scryptLength);
console.log("derive outside:", String(storage.getStore()));
