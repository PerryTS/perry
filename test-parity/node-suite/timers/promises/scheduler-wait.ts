import { scheduler } from "node:timers/promises";
await scheduler.wait(0);
console.log("scheduler wait done");
