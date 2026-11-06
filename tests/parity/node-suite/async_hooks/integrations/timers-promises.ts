import {
  setImmediate as immediate,
  setInterval as interval,
  setTimeout as delay,
  scheduler,
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
  await scheduler.yield();
  console.log("timers scheduler yield store:", storage.getStore());
  await scheduler.wait(1);
  console.log("timers scheduler wait store:", storage.getStore());
  for await (const value of interval(1, "interval-value")) {
    console.log("timers promise interval store:", storage.getStore(), value);
    break;
  }
  return "timers-result";
});

console.log("timers promise result:", result);
console.log("timers promise outside:", String(storage.getStore()));
