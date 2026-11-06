import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const snapshot = storage.run("captured", () => AsyncLocalStorage.snapshot());

storage.enterWith("caller");
const pending = snapshot(async (value: string) => {
  console.log("snapshot async start:", storage.getStore(), value);
  await Promise.resolve();
  console.log("snapshot async continuation:", storage.getStore());
  return value.toUpperCase();
}, "value");

console.log("snapshot async immediate restore:", storage.getStore());
console.log("snapshot async result:", await pending);
console.log("snapshot async final restore:", storage.getStore());

storage.disable();
