import { mock } from "node:test";

mock.timers.enable({ apis: ["setTimeout"], now: 0 });
const events: string[] = [];
const cancelled = setTimeout(() => events.push("cancelled"), 5);
setTimeout(() => events.push("kept"), 5);
clearTimeout(cancelled);
clearTimeout(undefined as any);
clearTimeout(null as any);
mock.timers.tick(5);
console.log("clear timeout:", JSON.stringify(events));
mock.timers.reset();
