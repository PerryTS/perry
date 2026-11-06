import { ReadableStream } from "node:stream/web";
// Strategy.size returning non-number — should error on enqueue.
const strategy = {
  size: () => "not-a-number" as any,
  highWaterMark: 1,
};
let enqueueErr: string | null = null;
const rs = new ReadableStream(
  {
    start(c) {
      try {
        c.enqueue("x");
      } catch (e: any) {
        enqueueErr = e && `${e.name}:${e.code}`;
      }
    },
  },
  strategy,
);
console.log("enqueue err:", enqueueErr);
console.log("constructed:", rs instanceof ReadableStream);
