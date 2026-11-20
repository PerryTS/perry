import { Readable } from "node:stream";
// flatMap(asyncGenFn) — yields multiple per input via async generator.
const r = Readable.from([1, 2, 3]);
const out: number[] = [];
for await (const v of (r as any).flatMap(async function* (x: number) {
  yield x * 10;
  yield x * 100;
})) {
  out.push(v as number);
}
console.log("collected:", out.join(","));
