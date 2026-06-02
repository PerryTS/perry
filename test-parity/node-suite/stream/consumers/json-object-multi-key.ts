import { Readable } from "node:stream";
import { json } from "node:stream/consumers";
// JSON object with nested keys/values.
const r = Readable.from([`{"a":1,"b":"two","c":[3,4,5]}`]);
const result = await json(r) as any;
console.log("a:", result.a);
console.log("b:", result.b);
console.log("c[2]:", result.c[2]);
