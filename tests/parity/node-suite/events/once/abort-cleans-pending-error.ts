import { EventEmitter, once } from "node:events";

const ee = new EventEmitter();
const ac = new AbortController();

const p = once(ee, "ready", { signal: ac.signal }).catch((err: any) =>
  err.name + ":" + (err.code || "no-code")
);

ac.abort();
console.log("abort:", await p);

try {
  ee.emit("error", new Error("late"));
  console.log("late no throw");
} catch (err: any) {
  console.log("late error:", err?.message);
}
