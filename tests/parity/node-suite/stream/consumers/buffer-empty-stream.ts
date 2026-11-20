import { Readable } from "node:stream";
import { buffer } from "node:stream/consumers";
// buffer() on empty stream returns empty Buffer.
const r = Readable.from([]);
const result = await buffer(r);
console.log("isBuffer:", Buffer.isBuffer(result));
console.log("length:", result.length);
console.log("is empty:", result.length === 0);
