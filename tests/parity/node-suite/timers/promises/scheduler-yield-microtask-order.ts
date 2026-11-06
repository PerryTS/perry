import { scheduler } from "node:timers/promises";

const events: string[] = [];
const yielded = scheduler.yield().then(() => events.push("yield"));
Promise.resolve().then(() => events.push("micro"));
events.push("sync");

await yielded;
console.log(events.join(","));
