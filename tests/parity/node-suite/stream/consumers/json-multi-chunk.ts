import { Readable } from "node:stream";
import { json } from "node:stream/consumers";
// JSON split across multiple chunks — reassembled correctly.
const r = Readable.from([`{"name":`, `"perry","ver`, `sion":1}`]);
const result = await json(r) as any;
console.log("name:", result.name);
console.log("version:", result.version);
