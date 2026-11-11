import { EventEmitter, on } from "node:events";

const ee = new EventEmitter();
const ac = new AbortController();
ac.abort();

try {
  on(ee, "data", { signal: ac.signal });
  console.log("unexpected");
} catch (err: any) {
  console.log("on pre-abort:", err?.name, err?.code || "no-code");
}
