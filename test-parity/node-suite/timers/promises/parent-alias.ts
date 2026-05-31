import * as timers from "node:timers";
import * as timersPromises from "node:timers/promises";

console.log("timers keys includes promises:", Object.keys(timers).includes("promises"));
console.log("timers.promises type:", typeof (timers as any).promises);
console.log(
  "parent === submodule:",
  (timers as any).promises === timersPromises ? "true" : "false",
);
console.log("promise keys:", JSON.stringify(Object.keys((timers as any).promises).sort()));
console.log(
  "export identities:",
  [
    (timers as any).promises.setTimeout === timersPromises.setTimeout,
    (timers as any).promises.setImmediate === timersPromises.setImmediate,
    (timers as any).promises.setInterval === timersPromises.setInterval,
    (timers as any).promises.scheduler === timersPromises.scheduler,
  ].join(","),
);
console.log(
  "scheduler methods:",
  [
    typeof (timers as any).promises.scheduler.wait,
    typeof (timers as any).promises.scheduler.yield,
  ].join(","),
);
