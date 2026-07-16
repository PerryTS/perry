import { mock } from "node:test";

mock.timers.enable({ apis: ["Date", "setTimeout"], now: 100 });
const events: string[] = [];
setTimeout(() => {
  events.push(`outer:${Date.now()}`);
  setTimeout(() => events.push(`inner:${Date.now()}`), 5);
}, 10);

mock.timers.runAll();
console.log("runAll:", JSON.stringify(events));
mock.timers.reset();
