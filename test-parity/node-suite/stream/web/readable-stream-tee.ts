import { ReadableStream } from "node:stream/web";
// tee() returns a pair of independent ReadableStreams that each consume the source.
const src = new ReadableStream({
  start(c) { c.enqueue("x"); c.close(); },
});
const [a, b] = src.tee();
const ra = a.getReader();
const rb = b.getReader();
console.log("a:", (await ra.read()).value);
console.log("b:", (await rb.read()).value);
