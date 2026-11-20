import { Readable } from "node:stream";
// Generator that returns early via `return` — stream ends at that point.
function* gen() {
  yield 1;
  yield 2;
  return; // explicit return
  yield 3; // never reached
}
const r = Readable.from(gen());
const out: number[] = [];
for await (const v of r) out.push(v as number);
console.log("count:", out.length);
console.log("values:", out.join(","));
