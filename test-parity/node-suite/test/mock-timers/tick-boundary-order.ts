import { mock } from "node:test";

mock.timers.enable({ apis: ["setTimeout"], now: 0 });
const events: string[] = [];
setTimeout(() => events.push("late"), 10);
setTimeout(() => events.push("first-at-five"), 5);
setTimeout(() => events.push("second-at-five"), 5);

mock.timers.tick(4);
console.log("tick4:", JSON.stringify(events));
mock.timers.tick(1);
console.log("tick5:", JSON.stringify(events));
mock.timers.tick(5);
console.log("tick10:", JSON.stringify(events));
mock.timers.reset();
