import { TransformStream } from "node:stream/web";
// new TransformStream() with no transformer is an identity transform.
const ts = new TransformStream();
console.log("readable:", ts.readable instanceof ReadableStream);
console.log("writable:", ts.writable instanceof WritableStream);
