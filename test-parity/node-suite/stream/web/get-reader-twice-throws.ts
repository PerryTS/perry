import { ReadableStream } from "node:stream/web";
// Calling getReader() on a stream that already has a reader (locked)
// must throw a TypeError.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const _r1 = rs.getReader();
let caught: string | null = null;
try {
  rs.getReader();
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
console.log("locked:", rs.locked);
