import { Readable } from "node:stream";
// Readable.from accepts a sync generator.
function* gen() {
  yield 1;
  yield 2;
  yield 3;
}
const r = Readable.from(gen());
const out: number[] = [];
r.on("data", (n) => out.push(n));
r.on("end", () => console.log("sum:", out.reduce((s, n) => s + n, 0)));
