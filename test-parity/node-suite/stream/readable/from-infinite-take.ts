import { Readable } from "node:stream";
// Readable.from(infiniteGen).take(N) cuts the stream at N items, even
// when the source is unbounded.
async function* counter() {
  let i = 0;
  while (true) yield i++;
}
const r = Readable.from(counter());
const limited = r.take(3);
const out: any[] = [];
for await (const v of limited as any) {
  out.push(v);
}
console.log("took:", out.join(","));
