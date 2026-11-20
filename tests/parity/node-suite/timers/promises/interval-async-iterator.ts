import { setInterval } from "node:timers/promises";

const ac = new AbortController();
const values: string[] = [];
try {
  for await (const value of setInterval(1, "tick", { signal: ac.signal })) {
    values.push(String(value));
    if (values.length === 3) ac.abort();
  }
} catch (err: any) { console.log("interval abort:", err?.name, err?.code || "no-code"); }
console.log("values:", values.join(","));
