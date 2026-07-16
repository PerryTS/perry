import {
  setImmediate as immediate,
  setTimeout as delay,
} from "node:timers/promises";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const result = await storage.run("timers-promises", async () => {
  const timeoutValue = await delay(1, "timeout-value");
  console.log(
    "timers promise timeout store:",
    storage.getStore(),
    timeoutValue,
  );
  const immediateValue = await immediate("immediate-value");
  console.log(
    "timers promise immediate store:",
    storage.getStore(),
    immediateValue,
  );
  return "timers-result";
});

console.log("timers promise result:", result);
console.log("timers promise outside:", String(storage.getStore()));
