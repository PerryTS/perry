import { Readable } from "node:stream";
// Readable.from([Promise.resolve(1), Promise.resolve(2)]) — Node yields
// the resolved values (awaits each).
const r = Readable.from([Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)]);
const out: any[] = [];
for await (const v of r) out.push(v);
console.log("count:", out.length);
console.log("values:", out.join(","));
