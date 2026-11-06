import { ReadableStream } from "node:stream/web";
// ReadableStream.from accepts an iterable; a single-value iterable yields once.
const single = {
  [Symbol.iterator]() {
    let yielded = false;
    return {
      next() {
        if (!yielded) {
          yielded = true;
          return { value: "the-value", done: false };
        }
        return { value: undefined, done: true };
      },
    };
  },
};
const rs = (ReadableStream as any).from(single);
const reader = rs.getReader();
const a = await reader.read();
const b = await reader.read();
console.log("a value:", a.value, "done:", a.done);
console.log("b done:", b.done);
