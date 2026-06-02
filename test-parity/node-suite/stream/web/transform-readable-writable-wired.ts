import { TransformStream, ReadableStream, WritableStream } from "node:stream/web";
// TransformStream exposes .readable (ReadableStream) and .writable
// (WritableStream) sides; they're correctly typed instances.
const ts = new TransformStream();
console.log("readable instanceof ReadableStream:", ts.readable instanceof ReadableStream);
console.log("writable instanceof WritableStream:", ts.writable instanceof WritableStream);
console.log("readable !== writable:", ts.readable !== (ts.writable as any));
