import { mock } from "node:test";

mock.timers.enable({ apis: ["setInterval"], now: 0 });
const events: string[] = [];
const first = setInterval(() => events.push("first"), 2);
const second = setInterval(() => {
  events.push("second");
  clearInterval(second);
}, 2);
mock.timers.tick(2);
mock.timers.tick(2);
clearInterval(first);
console.log("self clear:", JSON.stringify(events));
mock.timers.reset();
