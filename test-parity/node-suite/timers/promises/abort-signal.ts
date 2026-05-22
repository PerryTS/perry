import { setTimeout as delay, setImmediate as immediate } from "node:timers/promises";

const ac = new AbortController();
const p = delay(50, "late", { signal: ac.signal }).then(v => "resolved:" + v, (err: any) => err.name + ":" + (err.code || "no-code"));
ac.abort();
console.log("timeout abort:", await p);
console.log("immediate value:", await immediate("ok"));
