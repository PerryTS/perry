import { Readable } from "node:stream";
// Readable.from(Set) iterates the set values in insertion order.
const s = new Set([10, 20, 30]);
const r = Readable.from(s);
const out: number[] = [];
for await (const v of r) {
  out.push(v as number);
}
console.log("values:", out.join(","));
