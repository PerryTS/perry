import { Readable } from "node:stream";
import { json } from "node:stream/consumers";
// Deeply-nested JSON object.
const r = Readable.from([`{"a":{"b":{"c":42}}}`]);
const result = await json(r) as any;
console.log("deep value:", result.a.b.c);
console.log("is 42:", result.a.b.c === 42);
