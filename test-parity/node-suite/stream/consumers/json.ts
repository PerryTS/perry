import { Readable } from "node:stream";
import { json } from "node:stream/consumers";
// stream/consumers.json(stream) consumes a JSON-encoded stream and resolves
// to the parsed value.
const r = Readable.from(['{"x":', "1}"]);
const obj = await json(r);
console.log("x:", (obj as any).x);
