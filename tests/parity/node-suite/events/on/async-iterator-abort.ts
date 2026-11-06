import { EventEmitter, on } from "node:events";

const ee = new EventEmitter();
const ac = new AbortController();
const seen: string[] = [];
try {
  const iter = on(ee, "data", { signal: ac.signal });
  ee.emit("data", "a");
  ee.emit("data", "b");
  for await (const args of iter) {
    seen.push(args.join("/"));
    if (seen.length === 2) ac.abort();
  }
} catch (err: any) { console.log("abort:", err?.name, err?.code || "no-code"); }
console.log("seen:", seen.join(","));
