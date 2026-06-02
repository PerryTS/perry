import { Readable } from "node:stream";
// Readable.from(generatorFunction) — passes a function, not an iterator;
// Node calls it once and iterates the result.
function* gen() {
  yield 1;
  yield 2;
}
const r = Readable.from(gen as any);
const out: any[] = [];
for await (const v of r) out.push(v);
console.log("yielded:", out.join(","));
