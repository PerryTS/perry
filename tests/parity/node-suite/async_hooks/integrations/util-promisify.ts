import { promisify } from "node:util";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

function callbackOperation(
  value: number,
  callback: (error: null, result: number) => void,
) {
  process.nextTick(() => {
    console.log("promisify callback store:", storage.getStore());
    callback(null, value * 2);
  });
}

const promisedOperation = promisify(callbackOperation);
const result = await storage.run("promisify", async () => {
  const value = await promisedOperation(21);
  console.log("promisify continuation store:", storage.getStore());
  return value;
});

console.log("promisify result:", result);
console.log("promisify outside:", String(storage.getStore()));
