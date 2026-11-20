import { Readable } from "node:stream";
// some(fn) on an empty stream returns false (no item to match).
const r = Readable.from([]);
const result = await (r as any).some((_x: any) => true);
console.log("some(empty) returns:", result);
