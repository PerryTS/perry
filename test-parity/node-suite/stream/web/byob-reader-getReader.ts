import { ReadableStream } from "node:stream/web";
// getReader({ mode: "byob" }) returns a BYOB reader on byte streams.
// Without type:'bytes', it should throw TypeError.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
let caught: string | null = null;
try {
  rs.getReader({ mode: "byob" } as any);
} catch (e: any) {
  caught = e && e.name;
}
console.log("byob on non-byte stream throws:", caught);
