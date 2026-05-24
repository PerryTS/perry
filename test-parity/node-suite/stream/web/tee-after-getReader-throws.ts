import { ReadableStream } from "node:stream/web";
// tee() on a stream that already has a reader (locked) — throws TypeError.
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
