import { ReadableStream } from "node:stream/web";
// tee() on a locked stream should throw a TypeError because the stream
// already has a reader.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const _r = rs.getReader();
let caught: string | null = null;
try {
  rs.tee();
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
