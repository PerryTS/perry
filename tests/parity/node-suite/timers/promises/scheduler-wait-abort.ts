import { scheduler } from "node:timers/promises";

const ac = new AbortController();
const p = scheduler.wait(50, { signal: ac.signal }).then(() => "resolved", (err: any) => err.name + ":" + (err.code || "no-code"));
ac.abort();
console.log("wait abort:", await p);
console.log("yield value:", await scheduler.yield());
