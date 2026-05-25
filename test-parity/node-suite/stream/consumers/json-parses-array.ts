import { Readable } from "node:stream";
import { json } from "node:stream/consumers";
// json() parses a streamed JSON document into the corresponding JS value.
const r = Readable.from([`[1, 2, 3]`]);
const result = await json(r);
console.log("isArray:", Array.isArray(result));
console.log("values:", (result as number[]).join(","));
