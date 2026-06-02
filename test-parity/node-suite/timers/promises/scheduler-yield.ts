import { scheduler } from "node:timers/promises";
await scheduler.yield();
console.log("scheduler yield done");
