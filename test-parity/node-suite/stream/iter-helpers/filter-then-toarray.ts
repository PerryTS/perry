import { Readable } from "node:stream";
// filter(fn).toArray() — chain filter then toArray.
const r = Readable.from([1, 2, 3, 4, 5]);
const result = await (r as any).filter((x: number) => x % 2 === 0).toArray();
console.log("result:", result.join(","));
console.log("is array:", Array.isArray(result));
