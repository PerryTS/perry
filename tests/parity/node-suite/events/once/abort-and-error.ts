import { EventEmitter, once } from "node:events";

const ee = new EventEmitter();
const ac = new AbortController();
const p = once(ee, "ready", { signal: ac.signal }).then(() => "resolved", (err: any) => err.name + ":" + (err.code || "no-code"));
ac.abort();
console.log("abort once:", await p);

const ee2 = new EventEmitter();
const p2 = once(ee2, "ready").then(() => "resolved", (err: any) => err.message);
ee2.emit("error", new Error("bad"));
console.log("error once:", await p2);
