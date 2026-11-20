import { Readable } from "node:stream";
// drop(N) where N > items — yields empty.
const r = Readable.from([1, 2, 3]);
const out: any[] = [];
for await (const v of (r as any).drop(100)) out.push(v);
console.log("count:", out.length);
console.log("is empty:", out.length === 0);
