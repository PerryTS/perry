import { ReadableStream } from "node:stream/web";
// A type:"bytes" ReadableStream can only enqueue Buffer/TypedArray/DataView chunks.
let caught: string | null = null;
const rs = new ReadableStream({
  type: "bytes",
  start(c) {
    try {
      c.enqueue("not-a-buffer" as any);
    } catch (e: any) {
      caught = e && `${e.name}:${e.code}`;
    }
  },
} as any);
console.log("constructed:", rs instanceof ReadableStream);
console.log("enqueue threw:", caught);
