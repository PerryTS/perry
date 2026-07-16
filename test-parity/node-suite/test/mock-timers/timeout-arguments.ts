import { mock } from "node:test";

mock.timers.enable({ apis: ["setTimeout"], now: 0 });
const calls: unknown[][] = [];
setTimeout((...args: unknown[]) => calls.push(args), 5, "a", 2, true);
mock.timers.tick(5);
console.log("timeout args:", JSON.stringify(calls));
mock.timers.reset();
