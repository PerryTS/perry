import { Readable } from "node:stream";
// find(asyncFn) — returns Promise<value>; stops at first match.
const r = Readable.from([1, 2, 3, 4]);
const result = await (r as any).find(async (x: number) => x > 2);
console.log("result:", result);
console.log("is 3:", result === 3);
