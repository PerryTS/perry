import { ReadableStream } from "node:stream/web";

let called = false;
let enqueued = false;
const rs = new ReadableStream({
  pull(c) {
    called = true;
    c.enqueue("x");
    enqueued = true;
  },
});

const reader = rs.getReader();
const p = reader.read();
console.log("called before await:", called);
console.log("enqueued before await:", enqueued);
const result = await p;
console.log("result:", result.value, result.done);
