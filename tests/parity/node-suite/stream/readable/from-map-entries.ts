import { Readable } from "node:stream";
// Readable.from(Map) iterates entries (since Map is iterable yielding [k, v]).
const m = new Map<string, number>([
  ["a", 1],
  ["b", 2],
]);
const r = Readable.from(m);
const out: string[] = [];
for await (const v of r) {
  out.push(JSON.stringify(v));
}
console.log("entries:", out.join("|"));
