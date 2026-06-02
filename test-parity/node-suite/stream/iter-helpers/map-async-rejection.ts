import { Readable } from "node:stream";
// readable.map(asyncFn) — when the async fn throws, the resulting iterable
// rejects and iteration ends.
const r = Readable.from([1, 2, 3]);
const mapped = r.map(async (x: number) => {
  if (x === 2) throw new Error("map-fail");
  return x * 10;
});
const out: number[] = [];
let err: string | null = null;
try {
  for await (const v of mapped as any) out.push(v as number);
} catch (e: any) {
  err = e && e.message;
}
console.log("out:", out.join(","));
console.log("err:", err);
