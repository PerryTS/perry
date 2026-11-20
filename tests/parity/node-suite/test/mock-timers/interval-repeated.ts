import { mock } from "node:test";

mock.timers.enable({ apis: ["setInterval"], now: 0 });
const events: number[] = [];
const interval = setInterval((value: number) => events.push(value), 2, 7);
mock.timers.tick(1);
mock.timers.tick(1);
mock.timers.tick(4);
clearInterval(interval);
console.log("interval repeated:", JSON.stringify(events));
mock.timers.reset();
