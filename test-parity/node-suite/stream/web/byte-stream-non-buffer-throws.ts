import { ReadableStream } from "node:stream/web";
// A type:"bytes" ReadableStream can only enqueue ArrayBuffer/views — string throws.
let caught: string | null = null;
const rs = new ReadableStream({
  type: "bytes",
  start(c) {
    try {
      c.enqueue("not-a-buffer" as any);
    } catch (e: any) {
      caught = e && e.name;
    }
  },
} as any);
console.log("constructed:", rs instanceof ReadableStream);
console.log("enqueue threw:", caught);
