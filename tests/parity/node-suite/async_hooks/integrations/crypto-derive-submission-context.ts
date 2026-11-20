import { pbkdf2, scrypt } from "node:crypto";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

function pendingPbkdf2(label: string): Promise<string> {
  return storage.run(label, () => new Promise<string>((resolve, reject) => {
    pbkdf2("password", "salt", 2, 16, "sha256", (error, key) => {
      if (error) return reject(error);
      resolve(String(storage.getStore()) + ":" + key.length);
    });
  }));
}

function pendingScrypt(label: string): Promise<string> {
  return storage.run(label, () => new Promise<string>((resolve, reject) => {
    scrypt("password", "salt", 16, (error, key) => {
      if (error) return reject(error);
      resolve(String(storage.getStore()) + ":" + key.length);
    });
  }));
}

// Submit every job before awaiting any result; print in submission order.
const first = pendingPbkdf2("pbkdf2-first");
const second = pendingPbkdf2("pbkdf2-second");
const third = pendingScrypt("scrypt-first");
const fourth = pendingScrypt("scrypt-second");
console.log("submission outside:", String(storage.getStore()));
console.log("first:", await first);
console.log("second:", await second);
console.log("third:", await third);
console.log("fourth:", await fourth);
console.log("delivery outside:", String(storage.getStore()));
