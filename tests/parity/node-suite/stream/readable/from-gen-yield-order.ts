import { Readable } from "node:stream";
// Generator yield order preserved through Readable.from + async-iter.
function* gen() {
  yield "alpha";
  yield "beta";
  yield "gamma";
  yield "delta";
}
const r = Readable.from(gen());
const out: string[] = [];
for await (const v of r) out.push(String(v));
console.log("order:", out.join(","));
