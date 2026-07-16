import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const snapshot = storage.run("captured", () => AsyncLocalStorage.snapshot());
storage.enterWith("caller");
try {
  snapshot(() => {
    console.log("snapshot throw store:", storage.getStore());
    throw new Error("snapshot-error");
  });
} catch (error) {
  console.log("snapshot throw error:", (error as Error).message);
}
console.log("snapshot throw restored:", storage.getStore());
storage.disable();
