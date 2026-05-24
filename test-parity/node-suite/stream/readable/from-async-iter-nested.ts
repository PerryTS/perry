import { Readable } from "node:stream";
// Readable.from(asyncIter) where iter yields then awaits and yields more.
async function* gen() {
  yield 1;
  await new Promise((r) => setImmediate(r));
  yield 2;
  await new Promise((r) => setImmediate(r));
  yield 3;
}
const r = Readable.from(gen());
const out: number[] = [];
for await (const v of r) out.push(v as number);
console.log("collected:", out.join(","));
