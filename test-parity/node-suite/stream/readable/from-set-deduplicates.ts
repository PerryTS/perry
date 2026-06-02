import { Readable } from "node:stream";
// Set automatically deduplicates; Readable.from(Set) preserves that.
const s = new Set(["a", "b", "a", "c", "b"]);
const r = Readable.from(s);
const out: string[] = [];
for await (const v of r) out.push(String(v));
console.log("count:", out.length);
console.log("values:", out.join(","));
