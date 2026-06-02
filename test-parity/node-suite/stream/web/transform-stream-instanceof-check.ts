import { TransformStream, ReadableStream, WritableStream } from "node:stream/web";
// TransformStream — not instanceof RS or WS; readable/writable ARE.
const ts = new TransformStream();
console.log("ts instanceof TS:", ts instanceof TransformStream);
console.log("ts instanceof RS:", ts instanceof ReadableStream);
console.log("ts instanceof WS:", ts instanceof WritableStream);
console.log("readable RS:", ts.readable instanceof ReadableStream);
console.log("writable WS:", ts.writable instanceof WritableStream);
