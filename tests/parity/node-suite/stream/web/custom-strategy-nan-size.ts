import { ReadableStream } from "node:stream/web";
// A queuing strategy whose size() returns NaN should error the stream
// when an enqueue is attempted.
const strategy = {
  size: () => NaN,
  highWaterMark: 1,
};
const rs = new ReadableStream({
  start(c) {
    try {
      c.enqueue("x");
      console.log("enqueue succeeded:", true);
    } catch (e: any) {
      console.log("enqueue threw:", e && e.name);
    }
  },
}, strategy);
console.log("constructed:", rs instanceof ReadableStream);
