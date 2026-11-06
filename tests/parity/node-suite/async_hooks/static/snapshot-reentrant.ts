import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const snapshot = storage.run("captured", () => AsyncLocalStorage.snapshot());

storage.enterWith("caller");

const result = snapshot(() => {
  console.log("outer snapshot store:", storage.getStore());
  return snapshot((value: string) => {
    console.log("inner snapshot store:", storage.getStore());
    return value.toUpperCase();
  }, "nested");
});

console.log("reentrant snapshot result:", result);
console.log("reentrant snapshot restored:", storage.getStore());

storage.disable();
