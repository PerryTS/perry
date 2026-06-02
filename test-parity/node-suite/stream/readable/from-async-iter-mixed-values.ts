import { Readable } from "node:stream";
// Readable.from(asyncGen yielding mixed value types).
async function* gen() {
  yield 1;
  yield "two";
  yield { three: 3 };
  yield [4, 5];
}
const r = Readable.from(gen());
const out: any[] = [];
for await (const v of r) out.push(typeof v);
console.log("types:", out.join(","));
