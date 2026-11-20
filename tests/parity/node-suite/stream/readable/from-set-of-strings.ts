import { Readable } from "node:stream";
// Readable.from(Set) — iterates Set values in insertion order.
const s = new Set(["alpha", "beta", "gamma"]);
const r = Readable.from(s);
const out: string[] = [];
for await (const v of r) out.push(String(v));
console.log("order:", out.join(","));
