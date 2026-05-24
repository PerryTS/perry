import { Readable } from "node:stream";
// Chained .take(N).filter(fn) — both helpers participate.
const r = Readable.from([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
const out: number[] = [];
for await (const v of (r as any).take(6).filter((x: number) => x % 2 === 0)) {
  out.push(v as number);
}
console.log("result:", out.join(","));
