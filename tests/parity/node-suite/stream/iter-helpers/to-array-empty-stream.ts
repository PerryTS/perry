import { Readable } from "node:stream";
// toArray() on empty stream — returns empty array.
const r = Readable.from([]);
const result = await (r as any).toArray();
console.log("is array:", Array.isArray(result));
console.log("length:", result.length);
console.log("is empty:", result.length === 0);
