import { Readable } from "node:stream";
// drop(0) — yields all original items.
const r = Readable.from([1, 2, 3, 4]);
const out: number[] = [];
for await (const v of (r as any).drop(0)) out.push(v as number);
console.log("out:", out.join(","));
