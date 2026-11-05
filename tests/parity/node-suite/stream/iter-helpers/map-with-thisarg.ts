import { Readable } from "node:stream";
// Iterator helpers accept { signal, concurrency } options but do NOT have
// a thisArg in the options bag — fn binds normally. Confirm: arrow fn
// referencing outer var works.
const r = Readable.from([1, 2, 3]);
const multiplier = 10;
const mapped = r.map((x: any) => (x as number) * multiplier);
const out: number[] = [];
for await (const v of mapped as any) out.push(v);
console.log("mapped:", out.join(","));
