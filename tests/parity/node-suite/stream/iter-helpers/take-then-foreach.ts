import { Readable } from "node:stream";
// take(N).forEach(fn) — chain take with forEach.
const r = Readable.from([1, 2, 3, 4, 5]);
const out: number[] = [];
await (r as any).take(3).forEach((x: number) => { out.push(x); });
console.log("out:", out.join(","));
