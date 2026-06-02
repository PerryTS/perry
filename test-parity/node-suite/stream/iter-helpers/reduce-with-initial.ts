import { Readable } from "node:stream";
// reduce(fn, initial) with explicit initial value.
const r = Readable.from([1, 2, 3, 4]);
const result = await (r as any).reduce((acc: number, x: number) => acc + x, 100);
console.log("result:", result);
console.log("is 110:", result === 110);
