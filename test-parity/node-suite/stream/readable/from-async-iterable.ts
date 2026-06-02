import { Readable } from "node:stream";
// Readable.from accepts an async generator / async iterable.
async function* gen() {
  yield "a";
  yield "b";
}
const r = Readable.from(gen());
const out: string[] = [];
for await (const c of r) out.push(String(c));
console.log("joined:", out.join(","));
