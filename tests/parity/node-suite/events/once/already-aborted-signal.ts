import { EventEmitter, once } from "node:events";

const ee = new EventEmitter();
const ac = new AbortController();
ac.abort();

try {
  await once(ee, "ready", { signal: ac.signal });
  console.log("unexpected");
} catch (err: any) {
  console.log("once pre-abort:", err?.name, err?.code || "no-code");
}
