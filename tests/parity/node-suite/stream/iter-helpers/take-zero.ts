import { Readable } from "node:stream";
// take(0) — yields zero items.
const r = Readable.from([1, 2, 3]);
const out: number[] = [];
for await (const v of (r as any).take(0)) out.push(v as number);
console.log("count:", out.length);
console.log("is empty:", out.length === 0);
