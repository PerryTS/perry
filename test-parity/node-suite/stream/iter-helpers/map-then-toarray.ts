import { Readable } from "node:stream";
// .map(fn).toArray() — chain map then toArray to a plain array.
const r = Readable.from([1, 2, 3]);
const result = await (r as any).map((x: number) => x * 10).toArray();
console.log("is array:", Array.isArray(result));
console.log("result:", result.join(","));
