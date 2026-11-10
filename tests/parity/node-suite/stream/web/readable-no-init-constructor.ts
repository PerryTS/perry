import { ReadableStream } from "node:stream/web";
// new ReadableStream() — no args; constructs an empty stream.
const rs = new ReadableStream();
console.log("constructed:", rs instanceof ReadableStream);
console.log("locked:", rs.locked);
// Cancel to clean up
await rs.cancel();
