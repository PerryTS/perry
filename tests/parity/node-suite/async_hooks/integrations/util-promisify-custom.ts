import { AsyncLocalStorage } from "node:async_hooks";
import { promisify } from "node:util";

const storage = new AsyncLocalStorage<string>();
function callbackOperation(
  _value: number,
  _callback: (error: null, result: number) => void,
) {
  throw new Error("default callback implementation must not run");
}

const customImplementation = async (value: number) => {
  console.log("custom promisify start store:", storage.getStore());
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("custom promisify continuation store:", storage.getStore());
  return value * 3;
};
callbackOperation[promisify.custom] = customImplementation;
const promisedOperation = promisify(callbackOperation);

const result = await storage.run("custom-promisify", () =>
  promisedOperation(14),
);
console.log(
  "custom promisify identity/result:",
  promisedOperation === customImplementation,
  result,
);
console.log("custom promisify outside:", String(storage.getStore()));
