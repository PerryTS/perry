import { Readable } from "node:stream";
// Generator with `return value` — the returned value is NOT yielded as a chunk.
function* gen() {
  yield 1;
  yield 2;
  return 999; // not yielded
}
const r = Readable.from(gen());
const out: number[] = [];
for await (const v of r) out.push(v as number);
console.log("count:", out.length);
console.log("values:", out.join(","));
console.log("no 999:", !out.includes(999));
