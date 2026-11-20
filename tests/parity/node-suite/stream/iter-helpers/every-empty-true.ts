import { Readable } from "node:stream";
// every(fn) on an empty stream returns true (vacuously).
const r = Readable.from([]);
const result = await (r as any).every((_x: any) => false);
console.log("every(empty) returns:", result);
